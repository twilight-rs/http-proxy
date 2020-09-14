# Rust syntax target, either x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, arm-unknown-linux-musleabi etc.
ARG RUST_TARGET="x86_64-unknown-linux-musl"
# Musl target, either x86_64-linux-musl, aarch64-linux-musl, arm-linux-musleabi, etc.
ARG MUSL_TARGET="x86_64-linux-musl"
# This ONLY works with defaults which is rather annoying
# but better than nothing
# Uses docker's own naming for architectures
# e.g. x86_64 -> amd64, aarch64 -> arm64v8, arm -> arm32v7
ARG FINAL_TARGET="amd64"

FROM alpine:latest as build
ARG RUST_TARGET
ARG MUSL_TARGET

RUN apk upgrade && \
    apk add curl gcc musl-dev && \
    curl -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain nightly -y

RUN source $HOME/.cargo/env && \
    if [ "$RUST_TARGET" != $(rustup target list --installed) ]; then \
        rustup target add $RUST_TARGET && \
        curl -L "https://musl.cc/$MUSL_TARGET-cross.tgz" -o /toolchain.tgz && \
        tar xf toolchain.tgz && \
        ln -s "/$MUSL_TARGET-cross/bin/$MUSL_TARGET-gcc" "/usr/bin/$MUSL_TARGET-gcc" && \
        ln -s "/$MUSL_TARGET-cross/bin/$MUSL_TARGET-ld" "/usr/bin/$MUSL_TARGET-ld" && \
        ln -s "/$MUSL_TARGET-cross/bin/$MUSL_TARGET-strip" "/usr/bin/actual-strip" && \
        mkdir -p /app/.cargo && \
        echo -e "[target.$RUST_TARGET]\nlinker = \"$MUSL_TARGET-gcc\"" > /app/.cargo/config; \
    else \
        echo "skipping toolchain as we are native" && \
        ln -s /usr/bin/strip /usr/bin/actual-strip; \
    fi

WORKDIR /app

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# We need a source directory so that it builds the dependencies and an empty
# binary.
RUN mkdir src/
RUN echo 'fn main() {}' > ./src/main.rs
RUN source $HOME/.cargo/env && \
    cargo build --release \
        --target="$RUST_TARGET"

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
RUN rm -f target/$RUST_TARGET/release/deps/twilight_http_proxy*
COPY ./src ./src

RUN source $HOME/.cargo/env && \
    cargo build --release \
        --target="$RUST_TARGET" && \
    cp target/$RUST_TARGET/release/twilight-http-proxy /twilight-http-proxy && \
    actual-strip /twilight-http-proxy

FROM docker.io/${FINAL_TARGET}/alpine:latest
ARG TARGET

WORKDIR /app

# And now copy the binary over from the build container. The build container is
# based on a heavy image.
COPY --from=build /twilight-http-proxy /app/twilight-http-proxy

CMD ./twilight-http-proxy
