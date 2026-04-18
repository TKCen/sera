# ── sera-gateway multi-stage build ────────────────────────────────────────────
# Produces a small Debian-slim image with the compiled `sera` binary.
#
# Build context is the rust/ workspace directory:
#   docker build -f rust/Dockerfile -t sera-gateway:latest rust/
#
# Or via docker compose (context: ./rust in compose file):
#   docker compose -f docker-compose.rust.yaml build sera-gateway

# ── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

WORKDIR /build

# Install build dependencies
# Note: using rustls (pure Rust TLS) — no libssl-dev needed at runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Copy full workspace source (build context = rust/)
COPY . .

# Build release binary — target the `sera-gateway` bin in sera-gateway crate
# (renamed from `sera` to avoid a workspace collision with the sera-cli `sera` binary)
RUN cargo build --release --bin sera-gateway -p sera-gateway

# ── Stage 2: Runner ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runner

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only the compiled binary
COPY --from=builder /build/target/release/sera-gateway /app/sera-gateway

# Create log directory
RUN mkdir -p /app/logs

EXPOSE 3001

HEALTHCHECK --interval=30s --timeout=10s --retries=5 --start-period=30s \
    CMD curl -f http://localhost:3001/api/health || exit 1

ENTRYPOINT ["/app/sera-gateway"]
