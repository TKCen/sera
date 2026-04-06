# ── sera-core-rs multi-stage build ────────────────────────────────────────────
# Produces a small Debian-slim image with only the compiled binary.
#
# Build from repo root:
#   docker build -f rust/Dockerfile -t sera-core-rs:latest .
#
# Or via docker compose:
#   docker compose build sera-core-rs

# ── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

WORKDIR /build

# Install build dependencies (rustls = no libssl-dev needed)
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Copy full workspace source
COPY rust/ .

# Build release binary
RUN cargo build --release -p sera-core

# ── Stage 2: Runner ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runner

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only the compiled binary
COPY --from=builder /build/target/release/sera-core /app/sera-core

EXPOSE 3002

HEALTHCHECK --interval=30s --timeout=10s --retries=5 --start-period=10s \
    CMD curl -f http://localhost:3002/api/health || exit 1

ENTRYPOINT ["/app/sera-core"]
