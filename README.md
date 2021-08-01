# twilight-http-proxy

`http-proxy` is a ratelimited HTTP proxy in front of the Discord API, making use
of [twilight].

### Using it

HTTP clients often support proxies, such as Ruby's [`Net::HTTP`]. Read into your
HTTP client to see how to use it.

`twilight_http` natively supports using `twilight_http_proxy`, so you can use it like
this:

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
ratelimiting itself), and will request over HTTP. If your proxy is configured
to listen via HTTPS, then don't use HTTP.

By default, the proxy will use the token provided in the `DISCORD_TOKEN` enviroment variable for all requests. You can bypass this by providing a different token in the `Authorization` header yourself.

### Running via Docker

Prebuilt Docker images are published on [Docker Hub].

```sh
$ docker run -itd -e DISCORD_TOKEN="my token" -p 3000:80 twilightrs/http-proxy
# Or with metrics enabled
$ docker run -itd -e DISCORD_TOKEN="my token" -p 3000:80 twilightrs/http-proxy:metrics
```

This will set the discord token to `"my token"` and map the bound port to port
3000 on the host machine.

Images come in multiple different variants for metrics, ARM, Discord API v8 and
v6. You can use these with their corresponding image tags found on the
[Docker Hub tags page][docker-hub-tags].

### Running via Cargo

Build the binary:

```sh
$ cargo build --release
$ DISCORD_TOKEN="my token" PORT=3000 ./target/release/twilight_http_proxy
```

This will set the discord token to `"my token"` and bind to port 3000.

You can configure the behaviour when using multiple tokens with these enviroment variables:

* `CLIENT_DECAY_TIMEOUT` (defaults to 1 hour) sets the timeout after which a HTTP client (and associated ratelimit information) will be dropped due to not being used anymore
* `CLIENT_CACHE_MAX_SIZE` (defaults to no limit) limits the amount of HTTP clients in the cache - if full, the least recently used client will be removed
* `CLIENT_REAP_INTERVAL` (defaults to 10 minutes) changes the interval at which clients will be checked for decay

## Grafana metrics
The http proxy can expose grafana metrics when compiled with the ``expose-metrics`` feature. These metrics are then available on the ``/metrics`` endpoint.
You can set the metrics key used for the histogram data by setting the ``METRIC_KEY`` environment variable.

The exported histogram includes timing percentiles, response status codes, request path and request method. Calls to the metrics endpoint itself are not included in the metrics.

[twilight]: https://github.com/twilight-rs/twilight
[`Net::HTTP`]: https://ruby-doc.org/stdlib-2.4.1/libdoc/net/http/rdoc/Net/HTTP.html#method-c-new
[Docker Hub]: https://hub.docker.com/r/twilightrs/http-proxy
[docker-hub-tags]: https://hub.docker.com/r/twilightrs/http-proxy/tags
