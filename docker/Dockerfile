# ── Stage 1: Build ─────────────────────────────────────────────────────
FROM rust:latest AS builder

ARG TARGETARCH

# Install musl tools, cmake, and C++ compiler for native-code crates
# (esaxx-rs, onig_sys, etc.) that need g++ when cross-compiling to musl.
RUN apt-get update && apt-get install -y musl-tools cmake g++ \
    && if [ "$TARGETARCH" = "arm64" ]; then \
         rustup target add aarch64-unknown-linux-musl; \
       else \
         rustup target add x86_64-unknown-linux-musl; \
       fi \
    && rm -rf /var/lib/apt/lists/*

# Tell cc-rs to use the host g++ for musl C++ builds (same arch).
ENV CXX_aarch64_unknown_linux_musl=g++ \
    CXX_x86_64_unknown_linux_musl=g++ \
    CC_aarch64_unknown_linux_musl=musl-gcc \
    CC_x86_64_unknown_linux_musl=musl-gcc

WORKDIR /build

COPY Cargo.toml Cargo.lock* ./
ARG CARGO_OPTS=""
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && if [ "$TARGETARCH" = "arm64" ]; then TARGET=aarch64-unknown-linux-musl; else TARGET=x86_64-unknown-linux-musl; fi \
    && cargo build --release --target $TARGET ${CARGO_OPTS} \
    && rm -rf src

COPY src/ src/
ARG CARGO_OPTS=""
RUN touch src/main.rs \
    && if [ "$TARGETARCH" = "arm64" ]; then TARGET=aarch64-unknown-linux-musl; else TARGET=x86_64-unknown-linux-musl; fi \
    && cargo build --release --target $TARGET ${CARGO_OPTS} \
    && cp target/$TARGET/release/isartor /build/isartor-bin

# ── Stage 2: Runtime ──────────────────────────────────────────────────
FROM gcr.io/distroless/static-debian12

COPY --from=builder /build/isartor-bin /isartor

# Default environment variables (override at deploy time).
ENV ISARTOR_HOST_PORT=0.0.0.0:8080
EXPOSE 8080

ENTRYPOINT ["/isartor"]
