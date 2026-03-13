# ── Stage 1: Build ─────────────────────────────────────────────────────
FROM rust:latest AS builder

ARG TARGETARCH

# Install musl tools for static linking.
RUN apt-get update && apt-get install -y musl-tools \
    && if [ "$TARGETARCH" = "arm64" ]; then \
         rustup target add aarch64-unknown-linux-musl; \
       else \
         rustup target add x86_64-unknown-linux-musl; \
       fi \
    && rm -rf /var/lib/apt/lists/*

ENV CC_aarch64_unknown_linux_musl=musl-gcc \
    CC_x86_64_unknown_linux_musl=musl-gcc

WORKDIR /build

# Cache dependencies in a separate layer.
# Create stub src/main.rs and benches/ so Cargo.toml parses cleanly.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src benches \
    && echo 'fn main() {}' > src/main.rs \
    && echo 'fn main() {}' > benches/cache_latency.rs \
    && if [ "$TARGETARCH" = "arm64" ]; then TARGET=aarch64-unknown-linux-musl; else TARGET=x86_64-unknown-linux-musl; fi \
    && cargo build --release --target $TARGET --no-default-features \
    && rm -rf src benches

# Build the real binary (without embedded-inference / candle ML crates).
COPY src/ src/
RUN touch src/main.rs \
    && if [ "$TARGETARCH" = "arm64" ]; then TARGET=aarch64-unknown-linux-musl; else TARGET=x86_64-unknown-linux-musl; fi \
    && cargo build --release --target $TARGET --no-default-features \
    && cp target/$TARGET/release/isartor /build/isartor-bin

# ── Stage 2: Runtime ──────────────────────────────────────────────────
FROM gcr.io/distroless/static-debian12

COPY --from=builder /build/isartor-bin /isartor

# Default environment variables (override at deploy time).
ENV ISARTOR_HOST_PORT=0.0.0.0:8080
EXPOSE 8080

ENTRYPOINT ["/isartor"]
