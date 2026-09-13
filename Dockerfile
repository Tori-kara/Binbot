# ------------------------------------------------------------------------------
# Stage 1: Chef Base (cargo-chef for intelligent dependency caching)
# ------------------------------------------------------------------------------
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /app

# ------------------------------------------------------------------------------
# Stage 2: Planner (analyzes dependencies to create recipe.json)
# ------------------------------------------------------------------------------
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

# ------------------------------------------------------------------------------
# Stage 3: Builder (cooks dependencies then compiles release binary)
# ------------------------------------------------------------------------------
FROM chef AS builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Pre-compile only crate dependencies.
# Render caches this layer across builds unless Cargo.lock changes!
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy application source code and migrations for compile-time embedding
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations

# Build the release binary with optimizations
RUN cargo build --release --bin binbot

# ------------------------------------------------------------------------------
# Stage 4: Minimal Runtime Image (~40MB)
# ------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install minimal TLS certificates and curl for health check probing
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Run as non-root user for security
RUN useradd -m -u 10001 appuser
USER appuser

# Copy compiled binary from builder
COPY --from=builder --chown=appuser:appuser /app/target/release/binbot /app/binbot

# Render Web Service default port
ENV PORT=10000
ENV RUST_LOG=info

EXPOSE 10000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:${PORT}/healthz || exit 1

ENTRYPOINT ["/app/binbot"]
