mod error;

use error::{ChunkingRequest, InvalidPath, RequestError, RequestIssue};
use http::{request::Parts, Method as HttpMethod};
use hyper::{
    body::Body,
    server::{conn::AddrStream, Server},
    service, Request, Response,
};
use snafu::ResultExt;
use std::{
    convert::TryFrom,
    env,
    error::Error,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};
use tracing::{debug, error, info, trace};
use tracing_log::LogTracer;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use twilight_http::{
    client::Client,
    request::{Method, RequestBuilder},
    routing::Path,
    API_VERSION,
};

#[cfg(feature = "expose-metrics")]
use std::{future::Future, pin::Pin, sync::Arc, time::Instant};

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
    LogTracer::init()?;

    let log_filter_layer =
        EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;
    let log_fmt_layer = fmt::layer();

    let log_subscriber = tracing_subscriber::registry()
        .with(log_filter_layer)
        .with(log_fmt_layer);

    tracing::subscriber::set_global_default(log_subscriber)?;

    let host_raw = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let host = IpAddr::from_str(&host_raw)?;
    let port = env::var("PORT").unwrap_or_else(|_| "80".into()).parse()?;

    let client = Client::new(env::var("DISCORD_TOKEN")?);

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
        debug!("Connection from: {:?}", addr);
        let client = client.clone();

        #[cfg(feature = "expose-metrics")]
        let handle = handle.clone();

        async move {
            Ok::<_, RequestError>(service::service_fn(move |incoming: Request<Body>| {
                #[cfg(feature = "expose-metrics")]
                {
                    let uri = incoming.uri();

                    if uri.path() == "/metrics" {
                        handle_metrics(handle.clone())
                    } else {
                        Box::pin(handle_request(client.clone(), incoming))
                    }
                }

                #[cfg(not(feature = "expose-metrics"))]
                {
                    handle_request(client.clone(), incoming)
                }
            }))
        }
    });

    let server = Server::bind(&address).serve(service);

    info!("Listening on http://{}", address);

    if let Err(why) = server.await {
        error!("Fatal server error: {}", why);
    }

    Ok(())
}

fn path_name(path: &Path) -> &'static str {
    match path {
        Path::ChannelsId(..) => "Channel",
        Path::ChannelsIdInvites(..) => "Channel invite",
        Path::ChannelsIdMessages(..) => "Channel message",
        Path::ChannelsIdMessagesBulkDelete(..) => "Bulk delete message",
        Path::ChannelsIdMessagesId(..) => "Channel message",
        Path::ChannelsIdMessagesIdReactions(..) => "Message reaction",
        Path::ChannelsIdMessagesIdReactionsUserIdType(..) => "Message reaction for user",
        Path::ChannelsIdPermissionsOverwriteId(..) => "Channel permission override",
        Path::ChannelsIdPins(..) => "Channel pins",
        Path::ChannelsIdPinsMessageId(..) => "Specific channel pin",
        Path::ChannelsIdTyping(..) => "Typing indicator",
        Path::ChannelsIdWebhooks(..) => "Webhook",
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
        Path::WebhooksId(..) => "Webhook",
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
        _ => "Unknown path!",
    }
}

async fn handle_request(
    client: Client,
    request: Request<Body>,
) -> Result<Response<Body>, RequestError> {
    let api_url: String = format!("/api/v{}/", API_VERSION);
    debug!("Incoming request: {:?}", request);

    let (parts, body) = request.into_parts();
    let Parts {
        method,
        uri,
        headers,
        ..
    } = parts;

    let (method, m) = match method {
        HttpMethod::DELETE => (Method::Delete, "DELETE"),
        HttpMethod::GET => (Method::Get, "GET"),
        HttpMethod::PATCH => (Method::Patch, "PATCH"),
        HttpMethod::POST => (Method::Post, "POST"),
        HttpMethod::PUT => (Method::Put, "PUT"),
        _ => {
            error!("Unsupported HTTP method in request");
            return Err(RequestError::InvalidMethod { method });
        }
    };

    let trimmed_path = if uri.path().starts_with(&api_url) {
        uri.path().replace(&api_url, "")
    } else {
        uri.path().to_owned()
    };
    let path = Path::try_from((method, trimmed_path.as_ref())).context(InvalidPath)?;

    let bytes = (hyper::body::to_bytes(body).await.context(ChunkingRequest)?).to_vec();

    let path_and_query = match uri.path_and_query() {
        Some(v) => v.as_str().replace(&api_url, ""),
        None => {
            debug!("No path in URI: {:?}", uri);

            return Err(RequestError::NoPath { uri });
        }
    };
    let p = path_name(&path);
    let raw_request = RequestBuilder::raw(method, path, path_and_query)
        .body(bytes)
        .headers(headers.into_iter().filter_map(|(k, v)| k.map(|h| (h, v))))
        .build();

    #[cfg(feature = "expose-metrics")]
    let start = Instant::now();

    let resp = client.raw(raw_request).await.context(RequestIssue)?;

    #[cfg(feature = "expose-metrics")]
    let end = Instant::now();

    trace!("Response: {:?}", resp);

    #[cfg(feature = "expose-metrics")]
    histogram!(METRIC_KEY.as_str(), end - start, "method"=>m.to_string(), "route"=>p, "status"=>resp.status().to_string());

    debug!("{} {}: {}", m, p, resp.status());

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
