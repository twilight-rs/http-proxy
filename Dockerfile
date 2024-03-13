# Rust syntax target, either x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, arm-unknown-linux-musleabi etc.
ARG RUST_TARGET="x86_64-unknown-linux-musl"
# Musl target, either x86_64-linux-musl, aarch64-linux-musl, arm-linux-musleabi, etc.
ARG MUSL_TARGET="x86_64-linux-musl"
# The crate features to build this with
ARG FEATURES=""

FROM --platform=$BUILDPLATFORM rustlang/rust:nightly AS chef
ARG RUST_TARGET
ARG MUSL_TARGET
ARG FEATURES

RUN <<EOT
    set -ex
    apt-get update
    apt-get upgrade
    apt-get install --assume-yes musl-dev clang lld
EOT

RUN rustup target add $RUST_TARGET

RUN cargo install cargo-chef --locked

COPY <<EOF /app/.cargo/config.toml
[env]
CC_aarch64-unknown-linux-musl = "clang -target aarch64-unknown-linux-musl -fuse-ld=lld"
CXX_aarch64-unknown-linux-musl = "clang++ -target aarch64-unknown-linux-musl -fuse-ld=lld"
CC_x86_64-unknown-linux-musl = "clang -target x86_64-unknown-linux-musl -fuse-ld=lld"
CXX_x86_64-unknown-linux-musl = "clang++ -target x86_64-unknown-linux-musl -fuse-ld=lld"

[target.aarch64-unknown-linux-musl]
linker = "clang"
rustflags = [
          "-C", "link-args=-target aarch64-unknown-linux-musl -fuse-ld=lld",
          "-C", "strip", "symbols",
]

[target.x86_64-unknown-linux-musl]
linker = "clang"
rustflags = [
          "-C", "link-args=-target x86_64-unknown-linux-musl -fuse-ld=lld",
          "-C", "strip", "symbols",
]

[unstable]
build-std = ["std", "panic_abort"]
EOF

WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN <<EOF bash
    set -ex
    if test "$FEATURES" = ""; then
      cargo chef cook --target "$RUST_TARGET" --release --recipe-path recipe.json
    else
      cargo chef cook --target "$RUST_TARGET" --features="$FEATURES" --release --recipe-path recipe.json
    fi
EOF

COPY . .

RUN <<EOF bash
    set -ex
    if test "$FEATURES" = "" ; then
      cargo build --release --target $RUST_TARGET
    else
      cargo build --release --target $RUST_TARGET --features="$FEATURES"
    fi
    cp target/$RUST_TARGET/release/twilight-http-proxy /twilight-http-proxy
EOF


FROM scratch

COPY --from=builder /twilight-http-proxy /twilight-http-proxy

CMD ["./twilight-http-proxy"]
