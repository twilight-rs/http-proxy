mod error;
mod ratelimiter_map;
mod tlru;

use error::RequestError;
use http::{HeaderMap, HeaderValue, Method as HttpMethod, Uri, header};
use http_body_util::combinators::BoxBody;
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    service,
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
};
use ratelimiter_map::RatelimiterMap;
use std::{
    convert::Infallible,
    env,
    error::Error,
    net::{Ipv4Addr, SocketAddrV4},
    num::NonZero,
    pin::pin,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tokio_util::task::TaskTracker;
use tracing::{error, info, trace};
use twilight_http_ratelimiting::{Endpoint, Method, RateLimitHeaders, RateLimiter};

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

#[cfg(feature = "metrics")]
use http_body_util::{BodyExt, Full};
#[cfg(feature = "metrics")]
use metrics::histogram;
#[cfg(feature = "metrics")]
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
#[cfg(feature = "metrics")]
use metrics_util::MetricKindMask;
#[cfg(feature = "metrics")]
use std::{borrow::Cow, sync::LazyLock};

#[cfg(feature = "metrics")]
static METRIC_KEY: LazyLock<Cow<str>> = LazyLock::new(|| {
    env::var("METRIC_KEY").map_or(Cow::Borrowed("twilight_http_proxy"), Cow::Owned)
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let client = {
        let mut http_connector = HttpConnector::new();
        http_connector.enforce_http(false);

        let builder = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1();

        let https_connector = if env::var_os("DISABLE_HTTP2").is_some() {
            builder.wrap_connector(http_connector)
        } else {
            builder.enable_http2().wrap_connector(http_connector)
        };

        Client::builder(TokioExecutor::new()).build(https_connector)
    };

    #[cfg(feature = "metrics")]
    let handle = PrometheusBuilder::new()
        .idle_timeout(
            MetricKindMask::COUNTER | MetricKindMask::HISTOGRAM,
            Some(Duration::from_secs(
                parse_env("METRIC_TIMEOUT")?.unwrap_or(300),
            )),
        )
        .install_recorder()
        .expect("installed once");

    let ratelimiter_map = Arc::new(RatelimiterMap::new(
        env::var("DISCORD_TOKEN")?,
        Duration::from_secs(parse_env("CLIENT_DECAY_TIMEOUT")?.unwrap_or(3600)),
        parse_env("CLIENT_CACHE_MAX_SIZE")?.unwrap_or(NonZero::<usize>::MAX),
    ));

    let host = parse_env("HOST")?.unwrap_or(Ipv4Addr::UNSPECIFIED);
    let port = parse_env("PORT")?.unwrap_or(80);
    let address = SocketAddrV4::new(host, port);

    let listener = TcpListener::bind(&address).await?;
    let mut shutdown_signal = pin!(shutdown_signal());

    info!("Listening on http://{}", address);

    let tracker = TaskTracker::new();

    loop {
        tokio::select! {
            conn = listener.accept() => {
                let Ok((stream, addr)) = conn else {
                    error!("Failed to accept connection");
                    continue;
                };

                let ratelimiter_map = Arc::clone(&ratelimiter_map);
                let client = client.clone();
                #[cfg(feature = "metrics")]
                let handle = handle.clone();

                let service_fn = service::service_fn(move |request| {
                    let token = request
                        .headers()
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok());
                    let (ratelimiter, token) = ratelimiter_map.get_or_insert(token);
                    let client = client.clone();
                    #[cfg(feature = "metrics")]
                    let handle = handle.clone();

                    async move {
                        #[cfg(feature = "metrics")]
                        if request.uri().path() == "/metrics" {
                            return Ok::<_, Infallible>(handle_metrics(handle));
                        }

                        Ok::<_, Infallible>(handle_request(client, ratelimiter, token, request)
                            .await
                            .unwrap_or_else(|err| err.as_response()))
                    }
                });

                tracker.spawn(async move {
                    trace!("Connection from: {:?}", addr);

                    let result = Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(stream), service_fn)
                        .await;

                    if let Err(e) = result {
                        error!("Error serving {addr}: {e}");
                    }
                });
            },
            _ = shutdown_signal.as_mut() => {
                drop(listener);
                info!("Received shutdown signal, starting shutdown");
                break;
            }
        }
    }

    tracker.close();
    info!("waiting for {} task(s) to finish", tracker.len());
    tracker.wait().await;

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

async fn handle_request(
    client: Client<HttpsConnector<HttpConnector>, Incoming>,
    ratelimiter: RateLimiter,
    token: String,
    mut request: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, RequestError> {
    trace!("Incoming request: {:?}", request);

    let method = match *request.method() {
        HttpMethod::DELETE => Method::Delete,
        HttpMethod::GET => Method::Get,
        HttpMethod::PATCH => Method::Patch,
        HttpMethod::POST => Method::Post,
        HttpMethod::PUT => Method::Put,
        _ => {
            return Err(RequestError::InvalidMethod {
                method: request.into_parts().0.method,
            });
        }
    };

    let mut segments = request.uri().path().split("/").skip_while(|item| {
        matches!(*item, "" | "api")
            || item
                .strip_prefix("v")
                .is_some_and(|s| s.parse::<u8>().is_ok())
    });
    let mut path = segments.next().map_or(String::new(), ToOwned::to_owned);
    segments.for_each(|s| {
        path.push('/');
        path.push_str(s);
    });
    let endpoint = Endpoint { method, path };

    let permit = ratelimiter.acquire(endpoint).await;

    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_bytes(token.as_bytes())
            .expect("strings are guaranteed to be valid utf-8"),
    );
    request
        .headers_mut()
        .insert(header::HOST, HeaderValue::from_static("discord.com"));

    // Remove forbidden HTTP/2 headers
    // https://datatracker.ietf.org/doc/html/rfc7540#section-8.1.2.2
    request.headers_mut().remove(header::CONNECTION);
    request.headers_mut().remove("keep-alive");
    request.headers_mut().remove("proxy-connection");
    request.headers_mut().remove(header::TRANSFER_ENCODING);
    request.headers_mut().remove(header::UPGRADE);

    let prefix = if request.uri().path().starts_with("/api") {
        ""
    } else {
        "/api"
    };
    let mut uri_string = format!("https://discord.com{prefix}{}", request.uri().path());

    if let Some(query) = request.uri().query() {
        uri_string.push('?');
        uri_string.push_str(query);
    }

    let uri = Uri::from_str(&uri_string).expect("assembled from valid uri");
    *request.uri_mut() = uri;

    #[cfg(feature = "metrics")]
    let start = Instant::now();

    let resp = client
        .request(request)
        .await
        .map_err(|source| RequestError::RequestIssue { source })?;

    let end = Instant::now();

    let scope = resp
        .headers()
        .get(RateLimitHeaders::SCOPE)
        .map(HeaderValue::as_bytes);
    let headers = parse_headers(resp.headers(), scope, end);
    permit.complete(headers);

    trace!("Response: {:?}", resp);

    #[cfg(feature = "metrics")]
    {
        let scope = scope
            .and_then(|v| String::from_utf8(v.to_owned()).ok())
            .unwrap_or(String::new());
        let route = uri_string.split_off("https://discord.com".len());
        histogram!(METRIC_KEY.as_ref(), "method"=>method.name(), "route"=>route, "status"=>resp.status().to_string(), "scope" => scope)
            .record(end - start);
    }

    let (parts, body) = resp.into_parts();
    let boxed_body = BoxBody::new(body);
    let resp = Response::from_parts(parts, boxed_body);

    Ok(resp)
}

#[cfg(feature = "metrics")]
fn handle_metrics(handle: PrometheusHandle) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4"),
        )
        .body(BoxBody::new(
            Full::from(handle.render()).map_err(|_| unreachable!()),
        ))
        .unwrap()
}

fn parse_headers(
    headers: &HeaderMap,
    scope: Option<&[u8]>,
    start: Instant,
) -> Option<RateLimitHeaders> {
    match scope {
        Some(b"global") => {
            info!("globally rate limited");

            None
        }
        Some(b"shared") => {
            let bucket = headers.get(RateLimitHeaders::BUCKET)?.as_bytes().to_vec();
            let retry_after = headers
                .get(header::RETRY_AFTER)?
                .to_str()
                .ok()?
                .parse()
                .ok()?;

            Some(RateLimitHeaders {
                bucket,
                limit: 0,
                remaining: 0,
                reset_at: start + Duration::from_secs(retry_after),
            })
        }
        Some(b"user") => {
            let bucket = headers.get(RateLimitHeaders::BUCKET)?.as_bytes().to_vec();
            let limit = headers
                .get(RateLimitHeaders::LIMIT)?
                .to_str()
                .ok()?
                .parse()
                .ok()?;
            let remaining = headers
                .get(RateLimitHeaders::REMAINING)?
                .to_str()
                .ok()?
                .parse()
                .ok()?;
            let reset_after = headers
                .get(RateLimitHeaders::RESET_AFTER)?
                .to_str()
                .ok()?
                .parse()
                .ok()?;

            Some(RateLimitHeaders {
                bucket,
                limit,
                remaining,
                reset_at: start + Duration::from_secs_f32(reset_after),
            })
        }
        _ => None,
    }
}

fn parse_env<F>(key: &str) -> Result<Option<F>, Box<dyn Error>>
where
    F: FromStr,
    <F as FromStr>::Err: Error + 'static,
{
    match env::var(key) {
        Ok(s) => match s.parse() {
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(e.into()),
        },
        Err(env::VarError::NotPresent) => Ok(None),
        Err(e @ env::VarError::NotUnicode(_)) => Err(e.into()),
    }
}
