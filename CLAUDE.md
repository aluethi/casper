# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Casper

Multi-tenant AI agent platform: an OpenAI-compatible LLM gateway with tenant-scoped routing, retry/fallback, quota enforcement, and a stateful agent backend with a ReAct loop, composable prompt stacks, MCP integrations, and knowledge retrieval.

## Build & Lint Commands

### Rust (workspace root)
```bash
cargo build --workspace          # Build all crates
cargo test --workspace           # Run all tests
cargo clippy --workspace -- -D warnings  # Lint (CI treats warnings as errors)
cargo fmt --all -- --check       # Format check
cargo fmt --all                  # Auto-format
```

Single crate: `cargo test -p casper-base`, `cargo clippy -p casper-agent -- -D warnings`

### Frontend (web/)
```bash
cd web
npm ci                           # Install deps
npm run build                    # TypeScript check + Vite build (tsc -b && vite build)
npm run lint                     # ESLint
npm run dev                      # Dev server on :5173 (proxies /api, /auth, /v1 to :3000)
```

### Docker
```bash
docker compose up -d db          # Just PostgreSQL (for local dev)
docker compose up -d             # Full stack (db + server + searxng)
```

## Architecture

### Crate dependency graph
```
casper-base          Foundation: types, DB pools, JWT, auth, vault, observability
  └── casper-catalog   Model routing, quotas + LLM provider dispatch (Anthropic, OpenAI)
        └── casper-agent   ReAct engine, prompt assembly, tools, MCP client, delegation
casper-wire            WebSocket protocol types (shared between server and agent-backend sidecar)
```

### Binaries
- **casper-server** — Axum HTTP server, combines all library crates. Routes in `casper-server/src/routes/`, services in `casper-server/src/services/`
- **casper-cli** — CLI client
- **casper-agent-backend** — WebSocket sidecar for agent backend connections

### Frontend
- **web/** — React 18 + Vite + Tailwind + Zustand. SPA served through Caddy in production.

### Key data flow
```
Browser → Front Door (TLS) → Caddy (:80) → casper-server (:3000)
                                              ├── /api/v1/agents/:name/run → AgentEngine::run_stream
                                              ├── /api/v1/inference → dispatch_with_retry → Anthropic/OpenAI
                                              └── /agent/connect → WebSocket for agent backends
```

### Multi-tenancy
Row-level security (RLS) via PostgreSQL. `TenantDb` sets `app.tenant_id` config on every transaction. All queries run through RLS-scoped connections.

### Agent engine (casper-agent)
The ReAct loop in `engine/mod.rs`: loads agent config → assembles system prompt via `PromptContext` → calls LLM → dispatches tools → loops until end_turn or max_turns. Delegation to child agents via `DelegationRequest`. Streaming uses `RunStreamRequest`.

### LLM dispatch (casper-catalog)
`resolve_deployment` finds backend sequence by tenant + slug. `dispatch_with_retry` tries each backend with exponential backoff and fallback. Anthropic and OpenAI adapters normalize to a unified `LlmRequest`/`LlmResponse` format.

## Database

PostgreSQL 16 + pgvector. Migrations in `migrations/` run on server startup. Two connection pools: `main` (RLS-scoped) and `analytics`.

Local dev: `postgres://casper:casper@localhost:5432/casper` (port 5433 if using docker-compose).

## Configuration

Server config: `config/casper-server.yaml`. Overridable via env vars:
- `DATABASE_URL` — PostgreSQL connection string
- `CASPER_DEV_AUTH=true` — Enables dev mode with ephemeral keys and `/auth/dev-login`
- `CASPER_CONFIG` — Path to config YAML
- `RUST_LOG=info,sqlx=warn` — Log level

## Deployment

Azure VM behind Front Door. Bicep IaC in `infra/main.bicep`. GitHub Actions CD in `.github/workflows/deploy.yml`. Docker Compose on VM runs: Caddy (reverse proxy + SPA), casper-server, PostgreSQL, SearXNG.
