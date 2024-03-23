# Rust target, for example x86_64-unknown-linux-musl or aarch64-unknown-linux-musl.
ARG RUST_TARGET="x86_64-unknown-linux-musl"
# The crate features to build this with
ARG FEATURES=""

FROM --platform=$BUILDPLATFORM rustlang/rust:nightly AS builder
ARG RUST_TARGET
ARG FEATURES

RUN <<EOT
    set -ex
    apt-get update
    apt-get upgrade
    apt-get install --assume-yes musl-dev clang lld libgcc-12-dev-arm64-cross
EOT

RUN <<EOT
    set -ex
    cd /
    git clone -n --depth=1 --single-branch --filter=tree:0 https://github.com/llvm/llvm-project llvm
    cd /llvm
    git sparse-checkout set --no-cone compiler-rt
    git checkout
    cd /
EOT

RUN <<-EOT bash
    set -ex
    rustup target add "$RUST_TARGET"
    rustup component add rust-src --toolchain "nightly"
EOT

COPY <<-EOF /app/.cargo/config.toml
[env]
RUST_COMPILER_RT_ROOT="/llvm/compiler-rt"
CC_$RUST_TARGET = "clang -target $RUST_TARGET -fuse-ld=lld"
CXX_$RUST_TARGET = "clang++ -target $RUST_TARGET -fuse-ld=lld"

[target.$RUST_TARGET]
linker = "clang"
rustflags = [
          "-C", "link-args=-target $RUST_TARGET -fuse-ld=lld",
          "-C", "strip=symbols",
]

[unstable]
build-std = [
          "std",
          "panic_abort",
          "compiler_builtins",
          "compiler_builtins_c"
]
EOF

WORKDIR /app

COPY . .

RUN <<-EOT bash
    set -ex
    if test "$FEATURES" = "" ; then
      cargo build --release --target $RUST_TARGET
    else
      cargo build --release --target $RUST_TARGET --features="$FEATURES"
    fi
    cp target/$RUST_TARGET/release/twilight-http-proxy /twilight-http-proxy
EOT

FROM scratch

COPY --from=builder /twilight-http-proxy /twilight-http-proxy

CMD ["./twilight-http-proxy"]
