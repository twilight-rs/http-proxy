mod client_map;
mod error;

use client_map::ClientMap;
use const_format::concatcp;
use error::RequestError;
use http::{request::Parts, Method as HttpMethod, StatusCode};
use hyper::{
    body::Body,
    server::{conn::AddrStream, Server},
    service, Request, Response,
};
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

    let client_map: &'static ClientMap =
        Box::leak(Box::new(ClientMap::new(env::var("DISCORD_TOKEN")?)));

    let address = SocketAddr::from((host, port));

    #[cfg(feature = "expose-metrics")]
    let handle: &'static PrometheusHandle;

    #[cfg(feature = "expose-metrics")]
    {
        let recorder = PrometheusBuilder::new().build();
        handle = Box::leak(Box::new(recorder.handle()));
        metrics::set_boxed_recorder(Box::new(recorder))
            .expect("Failed to create metrics receiver!");
    }

    // The closure inside `make_service_fn` is run for each connection,
    // creating a 'service' to handle requests for that specific connection.
    let service = service::make_service_fn(move |addr: &AddrStream| {
        trace!("Connection from: {:?}", addr);

        async move {
            Ok::<_, RequestError>(service::service_fn(move |incoming: Request<Body>| {
                let token = incoming
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok());
                let client = client_map.get(token);

                #[cfg(feature = "expose-metrics")]
                {
                    let uri = incoming.uri();

                    if uri.path() == "/metrics" {
                        handle_metrics(handle)
                    } else {
                        Box::pin(handle_request(client, incoming))
                    }
                }

                #[cfg(not(feature = "expose-metrics"))]
                {
                    handle_request(client, incoming)
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
        _ => "Unknown path!",
    }
}

async fn handle_request(
    client: Client,
    request: Request<Body>,
) -> Result<Response<Body>, RequestError> {
    let api_url = concatcp!("/api/v", API_VERSION, "/");
    trace!("Incoming request: {:?}", request);

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
            error!("Unsupported HTTP method in request, {}", method);
            return Err(RequestError::InvalidMethod { method });
        }
    };

    let trimmed_path = if uri.path().starts_with(&api_url) {
        uri.path().replace(&api_url, "")
    } else {
        uri.path().to_owned()
    };

    let path = match Path::try_from((method, trimmed_path.as_ref())) {
        Ok(path) => path,
        Err(e) => {
            error!(
                "Failed to parse path for {:?} {}: {:?}",
                method, trimmed_path, e
            );
            return Err(RequestError::InvalidPath { source: e });
        }
    };

    let bytes = match hyper::body::to_bytes(body).await {
        Ok(body) => body.to_vec(),
        Err(e) => {
            error!("Failed to receive incoming request body: {:?}", e);
            return Err(RequestError::ChunkingRequest { source: e });
        }
    };

    let path_and_query = if let Some(v) = uri.path_and_query() {
        v.as_str().replace(&api_url, "")
    } else {
        error!("No path in URI: {:?}", uri);
        return Err(RequestError::NoPath { uri });
    };
    let p = path_name(&path);
    let raw_request = RequestBuilder::raw(method, path, path_and_query)
        .body(bytes)
        .headers(headers.into_iter().filter_map(|(k, v)| k.map(|h| (h, v))))
        .build();

    #[cfg(feature = "expose-metrics")]
    let start = Instant::now();

    let resp = match client.request::<Vec<u8>>(raw_request).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to receive reply body: {:?}", e);
            return Err(RequestError::RequestIssue { source: e });
        }
    };

    #[cfg(feature = "expose-metrics")]
    let end = Instant::now();

    trace!("Response: {:?}", resp);

    let status = resp.status();
    #[cfg(feature = "expose-metrics")]
    histogram!(METRIC_KEY.as_str(), end - start, "method"=>m.to_string(), "route"=>p, "status"=>status.to_string());

    let mut response_builder =
        Response::builder().status(StatusCode::from_u16(status.raw()).unwrap());

    for (header_name, header_value) in resp.headers() {
        response_builder = response_builder.header(header_name, header_value);
    }

    let reply = match resp.bytes().await {
        Ok(body) => match response_builder.body(Body::from(body)) {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to re-assemble body to reply with: {}", e);
                return Err(RequestError::ResponseAssembly { source: e });
            }
        },
        Err(e) => {
            error!("Failed to receive reply body: {:?}", e);
            return Err(RequestError::DeserializeBody { source: e });
        }
    };

    debug!("{} {} ({}): {}", m, p, uri.path(), status);

    Ok(reply)
}

#[cfg(feature = "expose-metrics")]
fn handle_metrics(
    handle: &'static PrometheusHandle,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, RequestError>> + Send>> {
    Box::pin(async move {
        Ok(Response::builder()
            .body(Body::from(handle.render()))
            .unwrap())
    })
}
