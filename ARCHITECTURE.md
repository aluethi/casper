# Casper Architecture

This document describes the internal architecture of the Casper platform. For a high-level overview, see [README.md](README.md). For the full specification, see [CASPER-SPEC-v1.md](CASPER-SPEC-v1.md).

## Crate Structure

Casper is organized as a Rust workspace with 12 crates. Dependencies flow downward -- no crate depends on one above it in this diagram.

```
                casper-base             <-- types, no DB
              /     |     \
     casper-db  casper-auth  casper-vault   <-- DB access, crypto
          |         |            |
     casper-observe  casper-llm         <-- business logic
          |              |
     casper-proxy   casper-knowledge        <-- external calls
          |              |
     casper-mcp     casper-agent            <-- agent runtime
               \    /
            casper-server                   <-- wires everything
            casper-cli                      <-- thin HTTP client
```

### Crate Descriptions

**casper-base** -- Shared foundation. Defines `TenantContext` (tenant ID + user ID + role + scopes carried through every request), `CasperError` (unified error type with HTTP status mapping), `JwtVerifier`, three-part scope parsing, and common types (`SecretValue`, pagination, etc.). Has no database dependency.

**casper-db** -- Database pool management. Provides `TenantDb`, a wrapper around `PgPool` that sets `app.current_tenant` on each connection before executing queries, enabling PostgreSQL row-level security. Also provides `DatabasePools` for holding the main, analytics, and owner pools.

**casper-auth** -- JWT signing with Ed25519 (via `ring`), token revocation cache (in-memory `DashMap` backed by `token_revocations` table). Handles the signing side; verification lives in `casper-base` so any crate can verify tokens without pulling in signing dependencies.

**casper-vault** -- Encrypted secret storage. Wraps AES-256-GCM encryption with HKDF-SHA256 key derivation. Each secret is encrypted with a unique derived key. Used for storing SSO client secrets, API provider credentials, and tenant secrets.

**casper-observe** -- Observability. `AuditWriter` batches audit log entries and flushes them to PostgreSQL on a timer. `UsageRecorder` tracks per-request token usage for billing and quota enforcement. `RuntimeMetrics` provides Prometheus-format metrics.

**casper-llm** -- Model and deployment management. Resolves which backend to call for a given deployment slug, applies retry/fallback chains, checks tenant quotas, and merges deployment-level parameter overrides with request parameters.

**casper-proxy** -- LLM provider dispatch. Contains `anthropic` and `openai` adapter modules that translate the internal `LlmRequest`/`LlmResponse` types to provider-specific wire formats. The `dispatch` function selects the adapter based on backend provider type.

**casper-knowledge** -- RAG pipeline. Handles document ingestion (upload, chunking), embedding generation (via casper-proxy), vector storage (pgvector), and similarity search retrieval. Used by the `knowledge_search` built-in agent tool.

**casper-mcp** -- Model Context Protocol client. Connects to external MCP servers, discovers available tools, forwards tool calls, and relays elicitation requests back to the user.

**casper-agent** -- Agent runtime. Contains the actor system (`ActorRegistry` manages per-conversation actors), the `AgentEngine` (drives the ReAct loop), the prompt assembler (builds system prompt from composable blocks), six built-in tools, and the reaper (cleans up idle actors).

**casper-server** -- Main binary. Wires all crates together into an Axum application. Defines all HTTP routes, authentication middleware, CORS, connection pool setup, migration runner, and graceful shutdown.

**casper-cli** -- CLI binary. A thin HTTP client that authenticates against casper-server and provides shell commands for common operations (tenant management, agent interaction, etc.).

## Data Flow Diagrams

### Inference Request Flow

An API key or JWT-authenticated request to the OpenAI-compatible inference endpoint:

```
Client
  |
  |  POST /v1/chat/completions
  |  Authorization: Bearer csk-...
  v
casper-server (Axum)
  |
  +--> auth middleware
  |      - API key: SHA-256 hash lookup in DB
  |      - JWT: Ed25519 signature verify + revocation check
  |      - Extract TenantContext (tenant_id, user_id, role, scopes)
  |
  +--> inference_routes::chat_completions
  |      |
  |      +--> casper-llm::resolve_deployment
  |      |      - Look up deployment by slug
  |      |      - Resolve backend chain (primary + fallbacks)
  |      |      - Check tenant quota (token budget)
  |      |      - Merge deployment params with request params
  |      |
  |      +--> casper-proxy::dispatch_with_retry
  |      |      - Select provider adapter (anthropic / openai)
  |      |      - Transform LlmRequest to provider wire format
  |      |      - HTTP POST to backend URL
  |      |      - Transform response back to LlmResponse
  |      |      - On failure: retry or try next backend in chain
  |      |
  |      +--> casper-observe::UsageRecorder::record
  |             - Record input/output tokens, model, latency
  |             - Debit tenant quota
  |
  +--> Return OpenAI-compatible JSON response
```

### Authentication Flow

#### Dev Login (development mode)

```
Client
  |
  |  POST /auth/dev-login {"email": "admin@ventoo.ch"}
  v
casper-server
  |
  +--> Find or create user by email
  +--> Generate Ed25519-signed JWT (ephemeral keys)
  +--> Return {access_token, refresh_token, expires_in}
```

#### OIDC Login (production)

```
Client (browser)
  |
  |  1. POST /auth/discover {"email": "user@acme.com"}
  v
casper-server
  |  Extract domain "acme.com" --> lookup email_domains --> find tenant
  |  Lookup sso_providers for tenant --> get OIDC issuer URL
  |  Return {auth_url, tenant_slug}
  |
  |  2. GET /auth/oidc/authorize?tenant=acme&...
  v
casper-server --> 302 Redirect to IdP authorization endpoint
  |
  |  3. User authenticates at IdP, redirected back
  |
  |  4. GET /auth/oidc/callback?code=...&state=...
  v
casper-server
  |  Exchange code for tokens at IdP
  |  Verify ID token, extract email + claims
  |  Find or create user in tenant
  |  Generate Casper JWT
  |  Return {access_token, refresh_token, expires_in}
```

### Agent ReAct Loop Flow

How an agent processes a user message:

```
Client
  |
  |  POST /api/v1/conversations/{id}/messages {"content": "..."}
  v
casper-server
  |
  +--> Lookup or create actor in ActorRegistry
  |      Actor = one conversation's in-memory state
  |
  +--> ActorHandle.send(message)
         |
         v
       AgentEngine.run()
         |
         +--> Load agent config from DB (deployment, system prompt, tools, etc.)
         |
         +--> Assemble prompt:
         |      1. Agent system prompt blocks (composable stack)
         |      2. Tenant memory (markdown document)
         |      3. Agent memory (markdown document)
         |      4. Tool definitions (built-in + MCP)
         |      5. Conversation history
         |      6. New user message
         |
         +--> ReAct Loop (max 25 turns):
         |      |
         |      +--> Call LLM (via catalog routing + proxy dispatch)
         |      |
         |      +--> If response is end_turn:
         |      |      Return final text to user
         |      |
         |      +--> If response contains tool_use blocks:
         |             For each tool call:
         |               |
         |               +--> ToolDispatcher.dispatch(tool_name, args)
         |               |      |
         |               |      +--> delegate:          spawn child agent
         |               |      +--> ask_user:           pause, wait for reply
         |               |      +--> knowledge_search:   pgvector similarity search
         |               |      +--> update_memory:      write to memory document
         |               |      +--> web_search:         query SearXNG
         |               |      +--> web_fetch:          HTTP GET + extract text
         |               |      +--> (MCP tool):         forward to MCP server
         |               |
         |               +--> Append tool results to conversation
         |               +--> Continue loop
         |
         +--> Record usage (tokens, tool calls, latency)
         +--> Write audit log entry
         +--> Return AgentResponse to caller
```

### Request Authentication Middleware

Every authenticated request passes through this middleware:

```
Incoming Request
  |
  +--> Extract Authorization header
  |
  +--> If "Bearer csk-..." (API key):
  |      SHA-256 hash the key
  |      Look up hash in api_keys table (with tenant join)
  |      Check key is active and not expired
  |      Build TenantContext from key's tenant_id, scopes, role
  |
  +--> If "Bearer csp_eyJ..." (JWT):
  |      Decode and verify Ed25519 signature
  |      Check expiry
  |      Check revocation cache (in-memory, DB fallback)
  |      Extract tenant_id, user_id, role, scopes from claims
  |      Build TenantContext
  |
  +--> Inject TenantContext into request extensions
  |
  +--> Route handler extracts TenantContext
  |      All DB queries scoped via RLS (SET app.current_tenant)
  |
  +--> Return response
```

## Database Design

### Row-Level Security (RLS)

All tenant-scoped tables use PostgreSQL RLS. The pattern:

1. Every tenant-scoped table has a `tenant_id UUID NOT NULL` column
2. An RLS policy checks `current_setting('app.current_tenant')::uuid = tenant_id`
3. The application role (`casper`) has RLS enabled; the owner role bypasses it
4. `TenantDb` sets `app.current_tenant` on each connection checkout

This means a bug in application code cannot accidentally leak data across tenants -- the database enforces isolation.

### Migration Strategy

Migrations are embedded in the server binary via `include_str!` and run on startup. A `_migrations` table tracks which migrations have been applied. The migration runner is idempotent -- re-running a migration that was already applied is a no-op.

Migration files:
- `0001_platform.sql` -- Platform tables (tenants, SSO, models, backends) -- no RLS
- `0002_tenant.sql` -- Tenant-scoped tables (users, agents, conversations, etc.) -- with RLS

### Key Tables

| Table               | RLS | Description                                    |
|---------------------|-----|------------------------------------------------|
| tenants             | No  | Tenant registry                                |
| sso_providers       | No  | OIDC provider config per tenant                |
| email_domains       | No  | Email domain to tenant mapping                 |
| models              | No  | Platform model catalog                         |
| platform_backends   | No  | LLM backend endpoints                         |
| users               | Yes | Tenant users                                   |
| api_keys            | Yes | API keys (SHA-256 hashed)                      |
| secrets             | Yes | Encrypted secrets (AES-256-GCM)                |
| deployments         | Yes | Model deployment routing rules                 |
| quotas              | Yes | Token usage quotas                             |
| agents              | Yes | Agent definitions with prompt stacks           |
| conversations       | Yes | Conversation threads                           |
| messages            | Yes | Conversation messages                          |
| runs                | Yes | Agent execution runs                           |
| knowledge_sources   | Yes | Uploaded documents                             |
| knowledge_chunks    | Yes | Document chunks with embeddings (pgvector)     |
| memories            | Yes | Agent and tenant memory documents              |
| usage_events        | Yes | Token usage records                            |
| audit_log           | Yes | Audit trail                                    |
| feedback            | Yes | Conversation quality feedback                  |

## Technology Choices

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Memory safety, performance, single binary deployment |
| Web framework | Axum 0.8 | Async, tower middleware ecosystem, type-safe extractors |
| Database | PostgreSQL 16 | RLS for tenant isolation, pgvector for embeddings, JSONB for flexible schemas |
| JWT signing | Ed25519 | Fast, small signatures, no RSA overhead |
| Secret encryption | AES-256-GCM + HKDF | Authenticated encryption, unique key per secret |
| Actor model | DashMap + tokio::spawn | Lightweight, no external actor framework needed |
| LLM dispatch | reqwest | Async HTTP client with streaming support |
| Tool protocol | MCP | Open standard for AI tool integration |
