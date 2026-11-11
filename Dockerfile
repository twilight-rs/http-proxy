# The crate features to build this with
ARG FEATURES=""

FROM docker.io/library/alpine:latest as build
ARG FEATURES

RUN apk upgrade && \
    apk add curl gcc musl-dev && \
    curl -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain nightly --component rust-src -y

WORKDIR /app

COPY . .

RUN source $HOME/.cargo/env && \
    cargo build \
        --release \
        -Zbuild-std=std,panic_abort \
        --target="$(uname -m)-unknown-linux-musl" --features="$FEATURES" && \
    cp target/$(uname -m)-unknown-linux-musl/release/twilight-http-proxy /twilight-http-proxy && \
    strip /twilight-http-proxy

FROM scratch

COPY --from=build /twilight-http-proxy /twilight-http-proxy

CMD ["./twilight-http-proxy"]
