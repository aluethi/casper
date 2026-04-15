# Casper

Casper is a multi-tenant AI agent platform built in Rust. It provides two product surfaces through a single server binary:

- **Inference API** -- An OpenAI-compatible LLM gateway with tenant-scoped routing, retry/fallback, quota enforcement, and usage tracking.
- **Agent API** -- A stateful agent backend with a ReAct loop, composable prompt stacks, built-in tools, MCP integrations, knowledge retrieval, and memory.

Casper is operated by Ventoo AG. All inference backends are platform-managed. Tenants access models via quotas allocated by the platform operator.

## Quick Start

### Prerequisites

- Docker and Docker Compose
- Rust toolchain (for local development)
- PostgreSQL 16 with pgvector (or use Docker Compose)

### Run with Docker Compose

```bash
# Start all services (PostgreSQL, Casper, SearXNG)
docker compose up -d

# Check health
curl http://localhost:3000/health

# Follow logs
docker compose logs -f app
```

### Run Locally (Development)

```bash
# 1. Start PostgreSQL (via Docker or system service)
docker compose up -d db

# 2. Copy and edit environment config
cp .env.example .env

# 3. Build and run
cargo build --workspace
cargo run -p casper-server

# 4. Health check
curl http://localhost:3000/health
# => {"status":"ok","db":"ok","version":"0.1.0"}
```

### Dev Login and First Steps

With `dev_auth: true` in the config, you can authenticate without SSO:

```bash
# Get a dev token
TOKEN=$(curl -s -X POST http://localhost:3000/auth/dev-login \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@ventoo.ch"}' | jq -r '.access_token')

# Create a tenant
curl -X POST http://localhost:3000/api/v1/tenants \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"slug":"acme","display_name":"Acme Corp"}'

# List agents (empty initially)
curl http://localhost:3000/api/v1/agents \
  -H "Authorization: Bearer $TOKEN"
```

## Architecture

```
+-----------------+     +------------------------------+
|   Portal (SPA)  |---->|       casper-server           |
|   React/Vite    |     |       (Rust / Axum)           |
+-----------------+     |                              |
                        |  +- Inference Layer --------+ |
+-----------------+     |  | Routing, retry/fallback, | |
|   casper-cli    |---->|  | provider dispatch         | |
|   (thin HTTP)   |     |  +--------------------------+ |
+-----------------+     |                              |
                        |  +- Agent Layer ------------+ |
+-----------------+     |  | Actors, ReAct loop,      | |
|  External MCP   |<---|  | prompt stack, delegation  | |
|  servers         |     |  +--------------------------+ |
+-----------------+     +--------------+---------------+
                                       |
                        +--------------+---------------+
                        |  PostgreSQL 16 + pgvector     |
                        |  + filesystem (knowledge)     |
                        +------------------------------+
```

## Project Structure

```
casper/
|-- crates/
|   |-- casper-base/          Shared types, JWT verification, RBAC scopes, errors
|   |-- casper-db/            PgPool, TenantDb, RLS wrapper
|   |-- casper-auth/          JWT signing, revocation cache
|   |-- casper-vault/         Secret vault (AES-256-GCM, HKDF)
|   |-- casper-observe/       Audit writer, usage tracking, Prometheus metrics
|   |-- casper-catalog/       Model catalog, backends, quotas, deployment routing
|   |-- casper-proxy/         LLM provider dispatch (Anthropic, OpenAI-compatible)
|   |-- casper-knowledge/     RAG: ingestion, chunking, embeddings, retrieval
|   |-- casper-mcp/           MCP client, tool discovery, elicitation forwarding
|   +-- casper-agent/         Actors, ReAct loop, prompt assembler, built-in tools
|
|-- casper-server/            Main binary: Axum routes, middleware, wiring
|-- casper-cli/               CLI binary: thin HTTP client for casper-server
|-- web/                      React SPA (portal UI)
|-- migrations/               SQL migration files
|-- config/                   Server configuration (YAML)
+-- data/                     Runtime data (knowledge files, not in git)
```

## Development Setup

### System Requirements

| Tool        | Version   |
|-------------|-----------|
| Rust        | 1.84+     |
| PostgreSQL  | 16+       |
| pgvector    | 0.7+      |
| Node.js     | 20+ (for web/) |

### Database Setup

If not using Docker Compose, create the database manually:

```bash
createdb casper
psql casper -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql casper -c "CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\";"
```

Migrations run automatically on server startup.

### Running Tests

```bash
# Unit and integration tests
cargo test --workspace

# With a test database
DATABASE_URL=postgres://casper:casper@localhost:5432/casper cargo test --workspace

# Clippy lints
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --all -- --check
```

### Docker Build

```bash
# Build the image
docker build -t casper-server .

# Run with custom config
docker run -p 3000:3000 \
  -v ./config/casper-server.yaml:/etc/casper/casper-server.yaml:ro \
  casper-server
```

## API Reference

Casper exposes these endpoint groups:

| Path                          | Description                       |
|-------------------------------|-----------------------------------|
| `GET /health`                 | Health check                      |
| `GET /metrics`                | Prometheus metrics                |
| `POST /auth/dev-login`        | Dev-mode login (dev_auth only)    |
| `GET /auth/oidc/authorize`    | OIDC authorization redirect       |
| `POST /auth/oidc/callback`    | OIDC callback                     |
| `GET /auth/status`            | Current session info              |
| `/api/v1/tenants`             | Tenant management                 |
| `/api/v1/users`               | User management                   |
| `/api/v1/api-keys`            | API key management                |
| `/api/v1/secrets`             | Encrypted secret vault            |
| `/api/v1/models`              | Model catalog                     |
| `/api/v1/backends`            | LLM backend configuration         |
| `/api/v1/deployments`         | Deployment routing rules          |
| `/api/v1/quotas`              | Tenant quota management           |
| `/v1/chat/completions`        | OpenAI-compatible inference       |
| `/api/v1/agents`              | Agent CRUD                        |
| `/api/v1/conversations`       | Conversation management           |
| `/api/v1/runs`                | Agent run execution               |
| `/api/v1/knowledge`           | Knowledge base management         |
| `/api/v1/memory`              | Agent/tenant memory               |
| `/api/v1/feedback`            | Conversation feedback             |
| `/api/v1/training`            | Training data export              |

See [API-v1.md](API-v1.md) for the full specification.

## Configuration

Server configuration is loaded from YAML. See [config/casper-server.yaml](config/casper-server.yaml) for a fully documented example.

Key configuration environment variables:

| Variable          | Description                              |
|-------------------|------------------------------------------|
| `CASPER_CONFIG`   | Path to YAML config (default: `config/casper-server.yaml`) |
| `RUST_LOG`        | Log level filter (default: `info,sqlx=warn`) |
| `DATABASE_URL`    | PostgreSQL connection string              |

## License

Proprietary. Copyright Ventoo AG.
