mod error;

use error::{ChunkingRequest, InvalidPath, RequestError, RequestIssue};
use http::request::Parts;
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
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};
use twilight_http::{
    client::Client, request::Request as TwilightRequest, routing::Path, API_VERSION,
};

#[cfg(feature = "expose-metrics")]
use std::{future::Future, pin::Pin, sync::Arc, time::Instant};

#[cfg(feature = "expose-metrics")]
use lazy_static::lazy_static;
#[cfg(feature = "expose-metrics")]
use metrics::timing;
#[cfg(feature = "expose-metrics")]
use metrics_core::{Builder, Drain};
#[cfg(feature = "expose-metrics")]
use metrics_runtime::{observers::PrometheusBuilder, Receiver};

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
    {
        let receiver = Receiver::builder()
            .build()
            .expect("Failed to create metrics receiver!");

        receiver.install();
    }

    #[cfg(feature = "expose-metrics")]
    let metrics_state = Arc::new(PrometheusBuilder::new());

    // The closure inside `make_service_fn` is run for each connection,
    // creating a 'service' to handle requests for that specific connection.
    let service = service::make_service_fn(move |addr: &AddrStream| {
        debug!("Connection from: {:?}", addr);
        let client = client.clone();

        #[cfg(feature = "expose-metrics")]
        let metrics_state = metrics_state.clone();

        async move {
            Ok::<_, RequestError>(service::service_fn(move |incoming: Request<Body>| {
                #[cfg(feature = "expose-metrics")]
                {
                    let uri = incoming.uri();

                    if uri.path() == "/metrics" {
                        handle_metrics(metrics_state.clone())
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

    let trimmed_path = if uri.path().starts_with(&api_url) {
        uri.path().replace(&api_url, "")
    } else {
        uri.path().to_owned()
    };
    let path = Path::try_from((method.clone(), trimmed_path.as_ref())).context(InvalidPath)?;

    let bytes = (hyper::body::to_bytes(body).await.context(ChunkingRequest)?).to_vec();

    let path_and_query = match uri.path_and_query() {
        Some(v) => v.as_str().replace(&api_url, "").into(),
        None => {
            debug!("No path in URI: {:?}", uri);

            return Err(RequestError::NoPath { uri });
        }
    };
    let body = if bytes.is_empty() { None } else { Some(bytes) };
    let p = path_name(&path);
    let m = method.to_string();
    let raw_request = TwilightRequest {
        body,
        form: None,
        headers: Some(headers),
        method,
        path,
        path_str: path_and_query,
    };

    #[cfg(feature = "expose-metrics")]
    let start = Instant::now();

    let resp = client.raw(raw_request).await.context(RequestIssue)?;

    #[cfg(feature = "expose-metrics")]
    let end = Instant::now();

    trace!("Response: {:?}", resp);

    #[cfg(feature = "expose-metrics")]
    timing!(METRIC_KEY, start, end, "method"=>m.to_string(), "route"=>p, "status"=>resp.status().to_string());

    debug!("{} {}: {}", m, p, resp.status());

    Ok(resp)
}

#[cfg(feature = "expose-metrics")]
fn handle_metrics(
    metrics_state: Arc<PrometheusBuilder>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, RequestError>> + Send>> {
    Box::pin(async move {
        Ok(Response::builder()
            .body(Body::from(metrics_state.build().drain()))
            .unwrap())
    })
}
