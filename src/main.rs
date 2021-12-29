mod error;
mod ratelimiter_map;

use error::RequestError;
use http::{
    header::{AUTHORIZATION, CONNECTION, HOST, TRANSFER_ENCODING, UPGRADE},
    HeaderValue, Method as HttpMethod, Uri,
};
use hyper::{
    body::Body,
    server::{conn::AddrStream, Server},
    service, Client, Request, Response,
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_trust_dns::{new_trust_dns_http_connector, TrustDnsHttpConnector};
use ratelimiter_map::RatelimiterMap;
use std::{
    convert::TryFrom,
    env,
    error::Error,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::Arc,
};
use tracing::{debug, error, info, trace};
use tracing_subscriber::EnvFilter;
use twilight_http_ratelimiting::{
    InMemoryRatelimiter, Method, Path, RatelimitHeaders, Ratelimiter,
};

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

#[cfg(feature = "expose-metrics")]
use std::{future::Future, pin::Pin, time::Instant};

#[cfg(feature = "expose-metrics")]
use lazy_static::lazy_static;
#[cfg(feature = "expose-metrics")]
use metrics::histogram;
#[cfg(feature = "expose-metrics")]
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

#[cfg(feature = "expose-metrics")]
lazy_static! {
    static ref METRIC_KEY: String =
        env::var("METRIC_KEY").unwrap_or_else(|_| "twilight_http_proxy".into());
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let host_raw = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let host = IpAddr::from_str(&host_raw)?;
    let port = env::var("PORT").unwrap_or_else(|_| "80".into()).parse()?;

    let https_connector = {
        let mut http_connector = new_trust_dns_http_connector();
        http_connector.enforce_http(false);

        let builder = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1();

        if env::var("DISABLE_HTTP2").is_ok() {
            builder.wrap_connector(http_connector)
        } else {
            builder.enable_http2().wrap_connector(http_connector)
        }
    };

    let client: Client<_, Body> = Client::builder().build(https_connector);
    let ratelimiter_map = Arc::new(RatelimiterMap::new(env::var("DISCORD_TOKEN")?));

    let address = SocketAddr::from((host, port));

    #[cfg(feature = "expose-metrics")]
    let handle: Arc<PrometheusHandle>;

    #[cfg(feature = "expose-metrics")]
    {
        let recorder = PrometheusBuilder::new().build();
        handle = Arc::new(recorder.handle());
        metrics::set_boxed_recorder(Box::new(recorder))
            .expect("Failed to create metrics receiver!");
    }

    // The closure inside `make_service_fn` is run for each connection,
    // creating a 'service' to handle requests for that specific connection.
    let service = service::make_service_fn(move |addr: &AddrStream| {
        trace!("Connection from: {:?}", addr);
        let ratelimiter_map = ratelimiter_map.clone();
        // Cloning a hyper client is fairly cheap by design
        let client = client.clone();

        #[cfg(feature = "expose-metrics")]
        let handle = handle.clone();

        async move {
            Ok::<_, RequestError>(service::service_fn(move |incoming: Request<Body>| {
                let token = incoming
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok());
                let (ratelimiter, token) = ratelimiter_map.get_or_insert(token);

                #[cfg(feature = "expose-metrics")]
                {
                    let uri = incoming.uri();

                    if uri.path() == "/metrics" {
                        handle_metrics(handle.clone())
                    } else {
                        Box::pin(handle_request(client.clone(), ratelimiter, token, incoming))
                    }
                }

                #[cfg(not(feature = "expose-metrics"))]
                {
                    handle_request(client.clone(), ratelimiter, token, incoming)
                }
            }))
        }
    });

    let server = Server::bind(&address).serve(service);

    let graceful = server.with_graceful_shutdown(shutdown_signal());

    info!("Listening on http://{}", address);

    if let Err(why) = graceful.await {
        error!("Fatal server error: {}", why);
    }

    Ok(())
}

#[cfg(windows)]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

#[cfg(unix)]
async fn shutdown_signal() {
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
    };
}

fn path_name(path: &Path) -> &'static str {
    match path {
        Path::ChannelsId(..) => "Channel",
        Path::ChannelsIdInvites(..) => "Channel invite",
        Path::ChannelsIdMessages(..) | Path::ChannelsIdMessagesId(..) => "Channel message",
        Path::ChannelsIdMessagesBulkDelete(..) => "Bulk delete message",
        Path::ChannelsIdMessagesIdReactions(..) => "Message reaction",
        Path::ChannelsIdMessagesIdReactionsUserIdType(..) => "Message reaction for user",
        Path::ChannelsIdPermissionsOverwriteId(..) => "Channel permission override",
        Path::ChannelsIdPins(..) => "Channel pins",
        Path::ChannelsIdPinsMessageId(..) => "Specific channel pin",
        Path::ChannelsIdTyping(..) => "Typing indicator",
        Path::ChannelsIdWebhooks(..) | Path::WebhooksId(..) => "Webhook",
        Path::Gateway => "Gateway",
        Path::GatewayBot => "Gateway bot info",
        Path::Guilds => "Guilds",
        Path::GuildsId(..) => "Guild",
        Path::GuildsIdBans(..) => "Guild bans",
        Path::GuildsIdAuditLogs(..) => "Guild audit logs",
        Path::GuildsIdBansUserId(..) => "Guild ban for user",
        Path::GuildsIdChannels(..) => "Guild channel",
        Path::GuildsIdWidget(..) => "Guild widget",
        Path::GuildsIdEmojis(..) => "Guild emoji",
        Path::GuildsIdEmojisId(..) => "Specific guild emoji",
        Path::GuildsIdIntegrations(..) => "Guild integrations",
        Path::GuildsIdIntegrationsId(..) => "Specific guild integration",
        Path::GuildsIdIntegrationsIdSync(..) => "Sync guild integration",
        Path::GuildsIdInvites(..) => "Guild invites",
        Path::GuildsIdMembers(..) => "Guild members",
        Path::GuildsIdMembersId(..) => "Specific guild member",
        Path::GuildsIdMembersIdRolesId(..) => "Guild member role",
        Path::GuildsIdMembersMeNick(..) => "Modify own nickname",
        Path::GuildsIdPreview(..) => "Guild preview",
        Path::GuildsIdPrune(..) => "Guild prune",
        Path::GuildsIdRegions(..) => "Guild region",
        Path::GuildsIdRoles(..) => "Guild roles",
        Path::GuildsIdRolesId(..) => "Specific guild role",
        Path::GuildsIdVanityUrl(..) => "Guild vanity invite",
        Path::GuildsIdWebhooks(..) => "Guild webhooks",
        Path::InvitesCode => "Invite info",
        Path::UsersId => "User info",
        Path::UsersIdConnections => "User connections",
        Path::UsersIdChannels => "User channels",
        Path::UsersIdGuilds => "User in guild",
        Path::UsersIdGuildsId => "Guild from user",
        Path::VoiceRegions => "Voice region list",
        Path::OauthApplicationsMe => "Current application info",
        Path::ChannelsIdMessagesIdCrosspost(..) => "Crosspost message",
        Path::ChannelsIdRecipients(..) => "Channel recipients",
        Path::ChannelsIdFollowers(..) => "Channel followers",
        Path::GuildsIdBansId(..) => "Specific guild ban",
        Path::GuildsIdMembersSearch(..) => "Search guild members",
        Path::GuildsIdTemplates(..) => "Guild templates",
        Path::GuildsIdTemplatesCode(..) => "Specific guild template",
        Path::GuildsIdVoiceStates(..) => "Guild voice states",
        Path::GuildsIdWelcomeScreen(..) => "Guild welcome screen",
        Path::WebhooksIdTokenMessagesId(..) => "Specific webhook message",
        Path::ApplicationCommand(..) => "Application commands",
        Path::ApplicationCommandId(..) => "Application command",
        Path::ApplicationGuildCommand(..) => "Application commands in guild",
        Path::ApplicationGuildCommandId(..) => "Application command in guild",
        Path::InteractionCallback(..) => "Interaction callback",
        Path::StageInstances => "Stage instances",
        Path::ChannelsIdMessagesIdThreads(_) => "Threads of a specific message",
        Path::ChannelsIdThreadMembers(_) => "Thread members",
        Path::ChannelsIdThreads(_) => "Channel threads",
        Path::GuildsIdStickers(_) => "Guild stickers",
        Path::GuildsTemplatesCode(_) => "Specific guild template",
        Path::GuildsIdThreads(_) => "Guild threads",
        Path::StickerPacks => "Sticker packs",
        Path::Stickers => "Stickers",
        Path::WebhooksIdToken(_, _) => "Webhook",
        _ => "Unknown path!",
    }
}

fn normalize_path(request_path: &str) -> (&str, &str) {
    if let Some(trimmed_path) = request_path.strip_prefix("/api") {
        if let Some(maybe_api_version) = trimmed_path.split('/').nth(1) {
            if let Some(version_number) = maybe_api_version.strip_prefix('v') {
                if version_number.parse::<u8>().is_ok() {
                    let len = "/api/v".len() + version_number.len();
                    return (&request_path[..len], &request_path[len..]);
                };
            };
        }

        ("/api", trimmed_path)
    } else {
        ("/api", request_path)
    }
}

async fn handle_request(
    client: Client<HttpsConnector<TrustDnsHttpConnector>, Body>,
    ratelimiter: InMemoryRatelimiter,
    token: String,
    mut request: Request<Body>,
) -> Result<Response<Body>, RequestError> {
    trace!("Incoming request: {:?}", request);

    let (method, m) = match *request.method() {
        HttpMethod::DELETE => (Method::Delete, "DELETE"),
        HttpMethod::GET => (Method::Get, "GET"),
        HttpMethod::PATCH => (Method::Patch, "PATCH"),
        HttpMethod::POST => (Method::Post, "POST"),
        HttpMethod::PUT => (Method::Put, "PUT"),
        _ => {
            error!("Unsupported HTTP method in request, {}", request.method());
            return Err(RequestError::InvalidMethod {
                method: request.into_parts().0.method,
            });
        }
    };

    let request_path = request.uri().path().to_owned();

    let (api_path, trimmed_path) = normalize_path(&request_path);

    let path = match Path::try_from((method, trimmed_path)) {
        Ok(path) => path,
        Err(e) => {
            error!(
                "Failed to parse path for {:?} {}: {:?}",
                method, trimmed_path, e
            );
            return Err(RequestError::InvalidPath { source: e });
        }
    };

    let p = path_name(&path);

    let header_sender = match ratelimiter.wait_for_ticket(path).await {
        Ok(sender) => sender,
        Err(e) => {
            error!("Failed to receive ticket for ratelimiting: {:?}", e);
            return Err(RequestError::AcquiringTicket { source: e });
        }
    };

    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_bytes(token.as_bytes())
            .expect("strings are guaranteed to be valid utf-8"),
    );
    request
        .headers_mut()
        .insert(HOST, HeaderValue::from_static("discord.com"));

    // Remove forbidden HTTP/2 headers
    // https://datatracker.ietf.org/doc/html/rfc7540#section-8.1.2.2
    request.headers_mut().remove(CONNECTION);
    request.headers_mut().remove("keep-alive");
    request.headers_mut().remove("proxy-connection");
    request.headers_mut().remove(TRANSFER_ENCODING);
    request.headers_mut().remove(UPGRADE);

    let mut uri_string = format!("https://discord.com{}{}", api_path, trimmed_path);

    if let Some(query) = request.uri().query() {
        uri_string.push('?');
        uri_string.push_str(query);
    }

    let uri = match Uri::from_str(&uri_string) {
        Ok(uri) => uri,
        Err(e) => {
            error!("Failed to create URI for requesting Discord API: {:?}", e);
            return Err(RequestError::InvalidURI { source: e });
        }
    };
    *request.uri_mut() = uri;

    #[cfg(feature = "expose-metrics")]
    let start = Instant::now();

    let resp = match client.request(request).await {
        Ok(response) => response,
        Err(e) => {
            error!("Error when requesting the Discord API: {:?}", e);
            return Err(RequestError::RequestIssue { source: e });
        }
    };

    let ratelimit_headers = RatelimitHeaders::from_pairs(
        resp.headers()
            .into_iter()
            .map(|(k, v)| (k.as_str(), v.as_bytes())),
    )
    .ok();

    if header_sender.headers(ratelimit_headers).is_err() {
        error!("Error when sending ratelimit headers to ratelimiter");
    };

    #[cfg(feature = "expose-metrics")]
    let end = Instant::now();

    trace!("Response: {:?}", resp);

    let status = resp.status();
    #[cfg(feature = "expose-metrics")]
        {
            let scope = resp.headers().get("X-RateLimit-Scope")
                .map(|header| header.to_str().unwrap_or("")).unwrap_or("")
                .to_string();
            histogram!(METRIC_KEY.as_str(), end - start, "method"=>m.to_string(), "route"=>p, "status"=>status.to_string(), "scope" => scope);
        }

    debug!("{} {} ({}): {}", m, p, request_path, status);

    Ok(resp)
}

#[cfg(feature = "expose-metrics")]
fn handle_metrics(
    handle: Arc<PrometheusHandle>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, RequestError>> + Send>> {
    Box::pin(async move {
        Ok(Response::builder()
            .body(Body::from(handle.render()))
            .unwrap())
    })
}
