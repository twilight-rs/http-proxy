FROM rustlang/rust:nightly-slim as build

RUN apt-get update
RUN apt-get install musl-tools -y
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# We need a source directory so that it builds the dependencies and an empty
# binary.
RUN mkdir src/
RUN echo 'fn main() {}' > ./src/main.rs
RUN RUSTFLAGS=-Clinker=musl-gcc cargo build --release \
    --target=x86_64-unknown-linux-musl

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
RUN rm -f target/x86_64-unknown-linux-musl/release/deps/twilight_http_proxy*
COPY ./src ./src

RUN RUSTFLAGS=-Clinker=musl-gcc cargo build --release \
    --target=x86_64-unknown-linux-musl

FROM alpine:latest

WORKDIR /app

# And now copy the binary over from the build container. The build container is
# based on a heavy image.
COPY --from=build \
    /app/target/x86_64-unknown-linux-musl/release/twilight-http-proxy \
    ./twilight-http-proxy

ENTRYPOINT ./twilight-http-proxy
