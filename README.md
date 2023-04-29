# twilight-http-proxy

`http-proxy` is a ratelimited HTTP proxy in front of the Discord API, making use
of [twilight].

## Use

This proxy is not a "real" HTTP `CONNECT` TCP proxy, it instead mocks the
Discord API.

Using it is as easy as changing the base of all routes from `discord.com` to
`localhost:3000`, like so:

```bash
# Previously
$ curl https://discord.com/api/users/@me
# With the proxy
$ curl http://localhost:3000/api/users/@me

# Or other API versions
$ curl http://localhost:3000/users/@me
$ curl http://localhost:3000/api/users/@me
$ curl http://localhost:3000/api/v8/users/@me
```

The proxy actively supports routes from API v10, but will also try to request
the corresponding routes on older or newer API versions if you so request in
the URL.

`twilight_http` natively supports using `twilight_http_proxy`, so you can use
it like this:

```rust
use twilight_http::Client;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = Client::builder()
        .proxy("localhost:3000", true)
        .ratelimiter(None)
        .build();

    Ok(())
}
```

This will use the running proxy, skip the ratelimiter (since the proxy does
ratelimiting itself), and will request over HTTP.

If you are using a different Discord API client, make sure that you are not
ratelimiting outgoing requests, because the proxy will do this instead. Very
short HTTP client timeouts may also cause issues with longer ratelimits.

### Multiple applications

By default, the proxy will use the token provided in the `DISCORD_TOKEN`
enviroment variable for all requests. You can bypass this by providing a
different token in the `Authorization` header yourself.

The proxy will keep track of ratelimits on a per-token basis, so using multiple
applications is as easy as sending the header alongside your requests.

You can configure how long the proxy stores ratelimit information with these
enviroment variables:

- `CLIENT_DECAY_TIMEOUT` (in seconds; defaults to 1 hour) sets the timeout
  after which ratelimiting information will be dropped due to not being used
  anymore
- `CLIENT_CACHE_MAX_SIZE` (defaults to no limit) limits the amount of
  ratelimiting information in the cache - if full, the least recently used
  ratelimiting information will be removed
- `METRIC_TIMEOUT` (in seconds; defaults to 5 minutes) controls how long
  metrics (metrics are identified by their combination of http method + route +
  response code + ratelimit scope) will continue to be reported past their last
  occurence before they are discarded. This avoids polluting your metrics with
  one off request metrics (9 datapoints per scrape) for long after it happened

### Running via Docker

| :exclamation:  The published images on Docker Hub will not work from April 14, 2023 due to Docker removing free team organizations! Use the new location described below. |
|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

Prebuilt Docker images are published on [Github's container registry].

```sh
$ docker run -itd -e DISCORD_TOKEN="my token" -p 3000:80 ghcr.io/twilight-rs/http-proxy
# Or with metrics enabled
$ docker run -itd -e DISCORD_TOKEN="my token" -p 3000:80 ghcr.io/twilight-rs/http-proxy:metrics
```

This will set the discord token to `"my token"` and map the bound port to port
3000 on the host machine.

Images come in two variants: The `metrics` tag is a build with prometheus metrics
support, the `latest` tag is a build without metrics support. Both support `x86_64`
as well as `aarch64` architectures.

### Running via Cargo

Build the binary:

```sh
$ cargo build --release
$ DISCORD_TOKEN="my token" PORT=3000 ./target/release/twilight-http-proxy
```

This will set the discord token to `"my token"` and bind to port 3000.

### Additional configuration

HTTP2 may cause issues with high concurrency (i.e. many concurrent requests).
If you encounter frequent error logs related to this, force the use of HTTP1 by
setting `DISABLE_HTTP2` to any value when running the proxy.

## Prometheus metrics

The HTTP proxy can expose prometheus metrics when compiled with the
`expose-metrics` feature. These metrics are then available on the `/metrics`
endpoint.
You can set the metrics key used for the histogram data by setting the
`METRIC_KEY` environment variable.

The exported histogram includes timing percentiles, response status codes,
request path and request method. Calls to the metrics endpoint itself are not
included in the metrics.

## Error behaviour

If processing an incoming request fails, the proxy will respond with a 5xx
status code and a helpful error message in the response body. Currently, these
status codes include:

- `500` if the proxy generates an invalid URI or the ratelimiter fails
  internally
- `501` if the client requested an unsupported API path or used an unsupported
  HTTP method
- `502` if the request made by the proxy fails

[twilight]: https://github.com/twilight-rs/twilight
[github's container registry]: https://github.com/twilight-rs/http-proxy/pkgs/container/http-proxy
