# The crate features to build this with
ARG FEATURES=""

FROM docker.io/library/alpine:latest as build
ARG FEATURES

RUN apk upgrade && \
    apk add curl gcc musl-dev && \
    curl -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain nightly --component rust-src -y

WORKDIR /app

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# We need a source directory so that it builds the dependencies and an empty
# binary.
RUN mkdir src/
RUN echo 'fn main() {}' > ./src/main.rs
RUN source $HOME/.cargo/env && \
    cargo build \
        --release \
        -Zbuild-std=std,panic_abort \
        --target="$(uname -m)-unknown-linux-musl" \
        --features="$FEATURES"

# Now, delete the fake source and copy in the actual source. This allows us to
# have a previous compilation step for compiling the dependencies, while being
# able to only copy in and compile the binary itself when something in the
# source changes.
#
# This is very important. If we just copy in the source after copying in the
# Cargo.lock and Cargo.toml, then every time the source changes the dependencies
# would have to be re-downloaded and re-compiled.
#
# Also, remove the artifacts of building the binaries.
RUN rm -f target/$(uname -m)-unknown-linux-musl/release/deps/twilight_http_proxy*
COPY ./src ./src

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
