# ── Stage 1: Build ─────────────────────────────────────────────────────
FROM rust:latest AS builder

# Install the musl target for fully static linking.
RUN rustup target add x86_64-unknown-linux-musl \
    && apt-get update \
    && apt-get install -y musl-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency compilation: copy manifests first, then build a
# dummy main so that `cargo build` downloads + compiles deps once.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --target x86_64-unknown-linux-musl \
    && rm -rf src

# Now copy the real source and build the final binary.
COPY src/ src/
RUN touch src/main.rs \
    && cargo build --release --target x86_64-unknown-linux-musl

# ── Stage 2: Runtime ──────────────────────────────────────────────────
FROM gcr.io/distroless/static-debian12

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/isartor /isartor

# Default environment variables (override at deploy time).
ENV ISARTOR_HOST_PORT=0.0.0.0:8080
EXPOSE 8080

ENTRYPOINT ["/isartor"]
