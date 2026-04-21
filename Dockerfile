# ── Frontend build stage ──────────────────────────────────────────
FROM node:22-slim AS frontend

WORKDIR /app

COPY web/package.json web/package-lock.json* ./
RUN npm ci

COPY web/ .
RUN npm run build

# ── Backend build stage ──────────────────────────────────────────
FROM rust:1.94 AS builder

WORKDIR /app

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/casper-base/Cargo.toml crates/casper-base/Cargo.toml
COPY crates/casper-catalog/Cargo.toml crates/casper-catalog/Cargo.toml
COPY crates/casper-agent/Cargo.toml crates/casper-agent/Cargo.toml
COPY crates/casper-wire/Cargo.toml crates/casper-wire/Cargo.toml
COPY casper-server/Cargo.toml casper-server/Cargo.toml
COPY casper-cli/Cargo.toml casper-cli/Cargo.toml
COPY casper-agent-backend/Cargo.toml casper-agent-backend/Cargo.toml

# Create stub source files so cargo can resolve the workspace and cache deps.
RUN mkdir -p crates/casper-base/src && echo "pub fn _stub(){}" > crates/casper-base/src/lib.rs && \
    mkdir -p crates/casper-catalog/src && echo "pub fn _stub(){}" > crates/casper-catalog/src/lib.rs && \
    mkdir -p crates/casper-agent/src && echo "pub fn _stub(){}" > crates/casper-agent/src/lib.rs && \
    mkdir -p crates/casper-wire/src && echo "pub fn _stub(){}" > crates/casper-wire/src/lib.rs && \
    mkdir -p casper-server/src && echo "fn main(){}" > casper-server/src/main.rs && \
    mkdir -p casper-cli/src && echo "fn main(){}" > casper-cli/src/main.rs && \
    mkdir -p casper-agent-backend/src && echo "fn main(){}" > casper-agent-backend/src/main.rs

# Pre-build dependencies (this layer is cached unless Cargo.toml/Cargo.lock change)
RUN cargo build --release -p casper-server 2>/dev/null || true

# Copy actual source code and migrations
COPY crates/ crates/
COPY casper-server/ casper-server/
COPY casper-cli/ casper-cli/
COPY casper-agent-backend/ casper-agent-backend/
COPY migrations/ migrations/

# Touch source files to invalidate the stub builds
RUN find crates/ casper-server/ casper-cli/ casper-agent-backend/ -name "*.rs" -exec touch {} +

# Build the real binary
RUN cargo build --release -p casper-server

# ── Runtime stage ─────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd --gid 1000 casper && \
    useradd --uid 1000 --gid casper --shell /bin/false casper

# Copy binary
COPY --from=builder /app/target/release/casper-server /usr/local/bin/casper-server

# Copy frontend build output
COPY --from=frontend /app/dist /var/lib/casper/web

# Copy default config
COPY config/casper-server.yaml /etc/casper/casper-server.yaml

# Create data directories
RUN mkdir -p /var/lib/casper/knowledge && chown -R casper:casper /var/lib/casper

ENV CASPER_CONFIG=/etc/casper/casper-server.yaml
ENV RUST_LOG=info,sqlx=warn

EXPOSE 3000

USER casper

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -sf http://localhost:3000/health || exit 1

CMD ["casper-server"]
