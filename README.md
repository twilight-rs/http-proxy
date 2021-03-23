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

### Running via Docker

Build the dockerfile and then run it:

```sh
docker build . -t http-proxy
# Or build with the metrics feature enabled
docker build . --build-arg FEATURES="expose-metrics" -t http-proxy

docker run -itd -e DISCORD_TOKEN="my token" -p 3000:80 http-proxy
```

This will set the discord token to `"my token"` and map the bound port to port
3000 on the host machine.

### Running via Cargo

Build the binary:

```sh
$ cargo build --release
$ DISCORD_TOKEN="my token" PORT=3000 ./target/release/twilight_http_proxy
```

This will set the discord token to `"my token"` and bind to port 3000.

## Grafana metrics
The http proxy can expose grafana metrics when compiled with the ``expose-metrics`` feature. These metrics are then available on the ``/metrics`` endpoint.
You can set the metrics key used for the histogram data by setting the ``METRIC_KEY`` environment variable.

The exported histogram includes timing percentiles, response status codes, request path and request method. Calls to the metrics endpoint itself are not included in the metrics.

[twilight]: https://github.com/twilight-rs/twilight
[`Net::HTTP`]: https://ruby-doc.org/stdlib-2.4.1/libdoc/net/http/rdoc/Net/HTTP.html#method-c-new
