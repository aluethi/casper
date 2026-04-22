# Casper Platform — Milestones & Tasks

**Companion to:** CASPER-SPEC-v1.md, API-v1.md, DATAMODEL-v1.md
**Purpose:** One task per Claude Code session. Work through in order.

---

## Ground Rules

1. Read CASPER-SPEC-v1.md, API-v1.md, DATAMODEL-v1.md first.
2. Read CLAUDE.md before every task.
3. Read BUILD_STATUS.md at the start of every task to understand context.
4. Execute each task as described. Use your judgement on details not specified.
5. After each task, run the verification command.
6. Only proceed to the next task if verification passes.
7. Commit and push after every task: `git add -A && git commit -m "Task XY — <description>" && git push origin main`
8. Append to BUILD_STATUS.md after every task (see ORCHESTRATION_PROMPT.md for format).
9. Rust edition 2024, PostgreSQL 16 with pgvector, Axum 0.8, SQLx 0.8.

---

## Phase 0 — Project Setup (4 tasks)

### Task 0A — Workspace and crate stubs

Create the workspace with all crate stubs. Every crate compiles with placeholder types.

```
casper/
├── Cargo.toml                    # workspace
├── crates/
│   ├── casper-base/
│   ├── casper-db/
│   ├── casper-auth/
│   ├── casper-vault/
│   ├── casper-observe/
│   ├── casper-llm/
│   ├── casper-proxy/
│   ├── casper-knowledge/
│   ├── casper-mcp/
│   └── casper-agent/
├── casper-server/
├── casper-cli/
├── web/                          # React SPA
│   └── e2e/                      # Playwright integration tests
├── migrations/
└── config/
    └── casper-server.yaml        # placeholder
```

**Verify:** `cargo build --workspace` succeeds.

---

### Task 0B — Database migration (platform tables)

Create `migrations/0001_platform.sql` with the 7 platform tables (no RLS):

`tenants`, `sso_providers`, `email_domains`, `models` (structured capability columns, cache pricing, published flag), `platform_backends` (provider + provider_label), `platform_backend_models`, `token_revocations`.

Enable pgvector: `CREATE EXTENSION IF NOT EXISTS vector`.

**Verify:** Run migration. `\dt` shows 7 tables.

---

### Task 0C — Database migration (tenant-scoped tables)

Create `migrations/0002_tenant.sql` with 19 tenant-scoped tables + RLS:

`current_tenant_id()` function. `tenant_users`, `api_keys`, `tenant_secrets`, `model_quotas` (cache fields), `model_deployments`, `agents`, `agent_state`, `agent_memory`, `agent_memory_history`, `tenant_memory`, `tenant_memory_history`, `snippets`, `conversations`, `messages`, `message_feedback`, `documents` (file_path), `document_chunks` (vector), `audit_log`, `usage_events` (cache token columns). RLS policies on all 19. All indexes. `conversation_quality` view.

**Verify:** `\dt` shows 26 tables. `\dv` shows 1 view. RLS blocks cross-tenant access.

---

### Task 0D — Server shell

Minimal Axum server in `casper-server`:

- Load config from YAML (database URL, port, cors_origins).
- Connect to PostgreSQL, run migrations.
- `GET /health` → `{ "status": "ok", "db": "ok", "version": "0.1.0" }`.
- CORS middleware, tracing subscriber, graceful shutdown.

**Verify:** `cargo run -p casper-server` starts, `curl localhost:3000/health` returns ok.

---

## Phase 1 — Foundation (11 tasks)

### Task 1A — Base types

In `casper-base`:
- `TenantId`, `CorrelationId` (UUID v7 newtypes).
- `Role` enum: `Viewer < Operator < Admin < Owner` with `PartialOrd`.
- `Subject` enum: `User(String)`, `ApiKey(String)`.
- `CasperError` enum with all variants, `status_code()`, `to_json()`.
- `SecretValue`: zeroizing, no Debug/Display/Serialize/Clone.
- `resolve_secret()` with `_FILE` convention.

**Verify:** `cargo test -p casper-base` — unit tests for role ordering, subject parsing, error mapping.

---

### Task 1B — Three-part scope matching

In `casper-base`:
- `Scope` struct with `grants(&self, requested: &Scope) -> bool`.
- Rules: exact match, two-part grants three-part, wildcard, `admin:*` grants all.
- `TenantContext`: tenant_id, subject, role, scopes, token_id, correlation_id.
- `require_scope()`, `require_role()` methods.

**Verify:** `cargo test -p casper-base` — tests: exact, two-part → three-part, wildcard, admin, denials.

---

### Task 1C — JWT verification

In `casper-base`:
- `CasperClaims` struct: sub, tid, role, scopes, exp, iat, iss, jti.
- `JwtVerifier` with Ed25519 public key. Verify signature, expiry, issuer.
- `authenticate()`: token → verify → build `TenantContext`.
- `RevocationCheck` trait (port).

**Verify:** `cargo test -p casper-base` — round-trip, expired rejected, bad signature rejected.

---

### Task 1D — JWT signing and revocation cache

In `casper-auth`:
- `JwtSigner` with Ed25519 private key.
- `RevocationCache`: `RwLock<HashSet<String>>` + background refresh from DB.
- `is_revoked(jti)`.

**Verify:** `cargo test -p casper-auth` — sign + verify, revocation prevents auth.

---

### Task 1E — Database layer

In `casper-db`:
- `DatabasePools`: main + analytics.
- `TenantDb`: acquire connection, `SET LOCAL app.tenant_id`.
- Pool creation from config.

**Verify:** `cargo test -p casper-db` — `TenantDb` enforces RLS.

---

### Task 1F — Secret vault

In `casper-vault`:
- HKDF-derived per-tenant keys from master key.
- `set()`, `get()` → `SecretValue`, `delete()`, `list_keys()`.
- `resolve_mcp_secrets()`: resolve `token_ref: "secret:key_name"`.

**Verify:** `cargo test -p casper-vault` — round-trip, tenant isolation, resolve token_ref.

---

### Task 1G — Audit writer

In `casper-observe`:
- `AuditWriter`: bounded channel (10,000), batched inserts.
- `log()`: actor, action, resource, detail, outcome, correlation_id, token_id.

**Verify:** `cargo test -p casper-observe` — entry written and queryable.

---

### Task 1H — Usage recorder and metrics

In `casper-observe`:
- `UsageRecorder`: writes `usage_events` with all four token types.
- `RuntimeMetrics`: Prometheus counters + histograms.
- `GET /metrics` handler.

**Verify:** Usage event recorded with cache tokens. Metrics endpoint returns prometheus format.

---

### Task 1I — JWT auth middleware

In `casper-server`:
- JWT middleware: extract Bearer → verify → revocation check → `TenantContext` as extension.
- Scope enforcement extractor: `RequireScope("agents:run")`.

**Verify:** Valid JWT passes. Invalid rejected. Scope enforcement blocks unauthorized.

---

### Task 1J — API key auth middleware

In `casper-server`:
- API key middleware: extract `csk-` → SHA-256 hash → lookup `api_keys` → build `TenantContext`.
- Three-part scopes from the key record flow into `TenantContext`.
- Unified auth layer: either JWT or API key, same `TenantContext` output.

**Verify:** Valid key passes with correct scopes. Invalid key rejected. Three-part scope check works.

---

### Task 1K — Auth routes

In `casper-server`:
- `POST /auth/login` (dev mode): email → find tenant_user → sign JWT pair.
- `POST /auth/refresh`: validate → rotate → return new pair.
- `POST /auth/logout`: add JTI to revocation list.
- `GET /auth/status`: return identity from `TenantContext`.

**Verify:** Dev login, refresh rotates, logout revokes, status returns identity.

---

## Phase 2 — Tenant & User Management (7 tasks)

### Task 2A — Tenant CRUD

- `POST /api/v1/tenants` — create tenant + owner user, return owner JWT.
- `GET /api/v1/tenants`, `GET /api/v1/tenants/:id`, `PATCH /api/v1/tenants/:id`.
- Scope: `platform:admin`.

**Verify:** Create, list, get, update. Non-admin rejected.

---

### Task 2B — SSO provider configuration

- `POST /api/v1/tenants/:id/sso` — configure OIDC provider. Client secret AES-encrypted.
- `GET /api/v1/tenants/:id/sso` — get config (secret never returned).
- `PATCH /api/v1/tenants/:id/sso` — update.
- `DELETE /api/v1/tenants/:id/sso` — remove.
- Scope: `platform:admin`.

**Verify:** Configure SSO for a tenant. Verify client_secret encrypted in DB. GET doesn't return secret.

---

### Task 2C — Email domain management

- `POST /api/v1/tenants/:id/domains` — add email domain.
- `GET /api/v1/tenants/:id/domains` — list.
- `DELETE /api/v1/tenants/:id/domains/:domain` — remove.
- Constraint: domain can only belong to one tenant (409 on conflict).
- Scope: `platform:admin`.

**Verify:** Add domain, verify uniqueness constraint, list, delete.

---

### Task 2D — OIDC flow (SSO start + callback)

- `POST /auth/sso/start` — email → extract domain → lookup tenant → lookup SSO provider → build OIDC URL with PKCE → return redirect.
- `GET /auth/sso/callback` — exchange authorization code, verify ID token, match user to `tenant_users`, issue JWT pair.
- Error: unknown domain (404), no SSO configured (404), user not pre-invited (403).

**Verify:** SSO start returns redirect URL for configured tenant. Mock the OIDC callback to verify token exchange and JWT issuance.

---

### Task 2E — User management

- `POST/GET/PATCH/DELETE /api/v1/users`.
- Three-part scopes in user records.
- Scope: `users:manage`.

**Verify:** Invite user with specific scopes, list, update role, deactivate.

---

### Task 2F — API keys

- `POST /api/v1/api-keys` — create, return `csk-` key once.
- `GET /api/v1/api-keys`, `GET /api/v1/api-keys/:id`.
- `PATCH /api/v1/api-keys/:id` — update name/scopes (key unchanged).
- `DELETE /api/v1/api-keys/:id` — deactivate.
- Three-part scopes: `["inference:sonnet-fast:call", "agents:triage:run"]`.

**Verify:** Create key, use it, verify scope enforcement (three-part allowed + denied). PATCH updates scopes.

---

### Task 2G — Secrets vault routes

- `POST /api/v1/secrets` — set (upsert).
- `GET /api/v1/secrets` — list keys only.
- `DELETE /api/v1/secrets/:key`.

**Verify:** Set, list (no values), delete.

---

## Phase 3 — Inference Layer (9 tasks)

### Task 3A — Model catalog (platform admin)

- `POST/GET/PATCH /api/v1/models` — all structured capability columns, cache pricing, published flag.
- Scope: `platform:admin`.

**Verify:** Create model, verify capabilities queryable. Unpublished not in tenant catalog.

---

### Task 3B — Platform backends

- `POST/GET/PATCH /api/v1/backends` — API key encrypted, never in GET.
- `POST /api/v1/models/:id/backends` — assign backend to model.
- `GET /api/v1/models/:id/backends`.
- `DELETE /api/v1/models/:id/backends/:backend_id`.

**Verify:** Create backend, assign, verify encryption, verify not in response.

---

### Task 3C — Model quotas

- `POST /api/v1/tenants/:id/quotas` — with cache limits + pricing overrides.
- `GET /api/v1/tenants/:id/quotas`.
- `PATCH /api/v1/tenants/:id/quotas/:model_id`.
- `DELETE /api/v1/tenants/:id/quotas/:model_id`.

**Verify:** Allocate quota with cache fields, verify appears in catalog.

---

### Task 3D — Tenant model catalog

- `GET /api/v1/catalog` — published models with quota status and deployment count.

**Verify:** Published + quota shows quota. Published without shows null. Unpublished absent.

---

### Task 3E — Model deployments

- `POST/GET/PATCH/DELETE /api/v1/deployments`.
- `POST /api/v1/deployments/:id/test` — dry-run routing.
- Validation: quota > 0, valid backends, unique slug.

**Verify:** Create, test routing, verify validation rejects invalid configs.

---

### Task 3F — Routing engine

In `casper-llm`:
- `resolve_deployment(tenant_id, slug)`.
- `select_backends(deployment)`.
- `dispatch_with_retry(deployment, backends, request)`.
- Retry + fallback logic.

**Verify:** Unit tests: resolution, retry with mock failures, fallback, all exhausted = 503.

---

### Task 3G — Anthropic provider adapter

In `casper-proxy`:
- Translate to Anthropic Messages API. Streaming (SSE). Tool use. Cache token extraction.

**Verify:** Wiremock: mock response, verify translation, streaming, tool_use, cache tokens.

---

### Task 3H — OpenAI-compatible provider adapter

In `casper-proxy`:
- Translate to OpenAI Chat Completions API. Streaming. Tool calling. Token counting fallback.

**Verify:** Wiremock: mock response, verify translation, streaming, tool calling.

---

### Task 3I — Inference endpoint

- `POST /v1/chat/completions` — auth, three-part scope check, resolve, quota, dispatch, stream, usage.
- `GET /v1/models` — returns deployments caller's scopes grant.

**Verify:** E2E with wiremock: model → backend → quota → deployment → scoped API key → call.

---

## Phase 4 — Knowledge & Memory (5 tasks)

### Task 4A — Knowledge document upload

In `casper-knowledge`:
- Write file to `data/knowledge/{tenant_id}/{document_id}.{ext}`.
- Text extraction (PDF, Markdown, plain text, HTML, DOCX).
- Chunking (~500 tokens with overlap).
- Store in `documents` + `document_chunks` (no embeddings yet).

Routes: `POST /api/v1/knowledge` (multipart), `GET`, `DELETE`.

**Verify:** Upload markdown, verify file on disk, chunks in DB.

---

### Task 4B — Knowledge embeddings and search

- `generate_embeddings(chunks)` via inference layer.
- Store in `document_chunks.embedding`.
- `search(query, limit)` — embed, cosine similarity, ranked results.

Route: `POST /api/v1/knowledge/search`.

**Verify:** Upload, search, verify relevant chunks returned.

---

### Task 4C — Agent memory CRUD

- `GET /api/v1/agents/:name/memory`, `PUT`, history list, history version.
- Version history with `updated_by`.

**Verify:** Update memory, verify version incremented, history has both versions.

---

### Task 4D — Tenant memory CRUD

- `GET /api/v1/tenant-memory`, `PUT`, history list, history version.

**Verify:** Same as agent memory.

---

### Task 4E — Snippets CRUD

- `POST/GET/PATCH/DELETE /api/v1/snippets`.
- `token_estimate` computed on write.

**Verify:** Create, verify token count, update, delete.

---

## Phase 5 — Agent Core (20 tasks)

### Task 5A — Agent CRUD

- `POST/GET/PATCH/DELETE /api/v1/agents/:name`.
- Validate `model_deployment` references active deployment.
- `prompt_stack` and `tools` stored as JSONB.
- Version incremented on update.

**Verify:** Create agent with full prompt_stack and all-object tools config, update, verify version.

---

### Task 5B — Prompt assembler: static blocks

In `casper-agent`:
- `text` blocks with `{{tenant.*}}` and `{{agent.*}}` template variables.
- `environment` blocks (datetime, tenant name, agent name, invocation source).
- `variable` blocks (key-value injection).
- `snippet` blocks (load from DB by ID).
- Token counting via `tiktoken-rs`.

**Verify:** Unit test: assemble with all static types, verify output and token counts.

---

### Task 5C — Prompt assembler: dynamic blocks

In `casper-agent`:
- `agent_memory` block: load from `agent_memory` table.
- `tenant_memory` block: load from `tenant_memory` table.
- `knowledge` block: placeholder (wired to real search later).
- `delegates` block: render available agents section.
- Budget allocation: static guaranteed, knowledge capped, history gets remainder.

**Verify:** Unit test: assemble with memory + delegates, verify budget allocation respects limits.

---

### Task 5D — Prompt assembler: datasource blocks

In `casper-agent`:
- Resolve `{{metadata.*}}` from run call metadata.
- MCP datasource: call MCP tool at assembly time.
- HTTP datasource: fetch URL at assembly time.
- `on_missing`: `"skip"` or `"fail"`.
- Budget cap on result.

**Verify:** Unit test with mock MCP: template variables resolve, datasource fetched, on_missing skip works.

---

### Task 5E — Message persistence and conversation routes

- Create conversation on first message (auto-title from first user message).
- Store messages: role, author, content JSONB, token_count.
- Routes: `GET /api/v1/conversations`, `GET /:id` (with messages), `DELETE`, `PATCH /:id/outcome`.

**Verify:** Create conversation, store messages, list, get with messages, set outcome.

---

### Task 5F — Conversation history loading

In `casper-agent`:
- Load conversation history for prompt assembly.
- Most recent turns first within budget.
- Truncation keeps tool_use/tool_result pairs intact (never split a pair).
- Budget-aware: stop adding turns when budget exhausted.

**Verify:** Unit test: 20 messages, budget for 10 — verify most recent kept, pairs intact.

---

### Task 5G — Built-in tool: update_memory

In `casper-agent`:
- `Tool` trait: `name()`, `description()`, `parameters_schema()`, `execute()`.
- `UpdateMemoryTool`: write new content to `agent_memory`, old to history, increment version.
- Enforce `max_document_tokens`.
- `ToolDispatcher`: register tools, resolve by name, execute.

**Verify:** Execute, memory updated, version in history, token limit enforced.

---

### Task 5H — Built-in tool: knowledge_search

In `casper-agent`:
- `KnowledgeSearchTool`: calls `casper-knowledge` search.
- Respects `max_results` and `relevance_threshold` from config.

**Verify:** Upload doc, search via tool, relevant chunks returned, respects config.

---

### Task 5I — Built-in tools: web_search and web_fetch

In `casper-agent`:
- `WebSearchTool`: calls SearXNG (from server config). Respects `max_results`.
- `WebFetchTool`: HTTP GET with `timeout_secs` and `max_response_bytes`. Truncation.

**Verify:** web_search returns results from mock SearXNG. web_fetch fetches URL, truncates.

---

### Task 5J — Built-in tools: delegate and ask_user (stubs)

In `casper-agent`:
- `DelegateTool`: returns sentinel in result metadata. Not yet wired to actor system.
- `AskUserTool`: returns sentinel in result metadata. Not yet wired to notification.
- Both registered in `ToolDispatcher`, resolvable by name.

**Verify:** Both execute and return sentinels. Dispatcher resolves them.

---

### Task 5K — Actor registry

In `casper-agent`:
- `ActorKey`: `(TenantId, String, Uuid)`.
- `ActorHandle`: `mpsc::Sender<ActorMessage>`, `last_activity`.
- `ActorRegistry`: `DashMap<ActorKey, ActorHandle>`.
- `get_or_activate()`: lookup or spawn tokio task with mailbox.
- Actor task: receive, process (placeholder), respond via oneshot.

**Verify:** Activate actor, send message, receive response. Second message reuses actor.

---

### Task 5L — Idle reaper

In `casper-agent`:
- Background task: scan registry, remove actors idle > timeout.
- Configurable timeout.
- `CancellationToken` for graceful shutdown.

**Verify:** Activate actor, wait > timeout, verify removed. New message reactivates.

---

### Task 5M — ReAct loop: structure

In `casper-agent`:
- `AgentEngine` drives the cycle:
  1. Load agent config.
  2. Assemble prompt.
  3. Call a mock LLM (returns canned responses for testing).
  4. If tool_use: dispatch via ToolDispatcher. Append results. Loop.
  5. If end_turn: append message, return.
  6. Max turns check.
- No real LLM call yet, no audit/usage.

**Verify:** Unit test with mock: message → tool_use → tool executes → end_turn → response returned.

---

### Task 5N — ReAct loop: LLM wiring and observability

In `casper-agent`:
- Replace mock LLM with real dispatch via `casper-llm` routing → `casper-proxy`.
- After each LLM call: record audit event + usage event (with cache tokens).
- Wire into actor mailbox processing.

**Verify:** E2E with wiremock: message → real LLM dispatch → tool → LLM → response. Audit + usage recorded.

---

### Task 5P — /run endpoint: sync mode

- `POST /api/v1/agents/:name/run` — auth, three-part scope check, resolve agent, get_or_activate actor, send to mailbox, await response.
- Source and metadata passed through to prompt assembler.
- Returns: message, usage (with cache tokens), correlation_id.

**Verify:** curl: send message to agent, get response with usage stats.

---

### Task 5Q — /run endpoint: async mode and draft config

- `async: true` → return 202 with task_id. Store result in DB when complete.
- `GET /api/v1/agents/:name/tasks/:id` — poll for result.
- `draft` field: override prompt_stack/tools/model/config without saving.

**Verify:** Send async, poll, get result. Send with draft config, verify draft used.

---

### Task 5R — Delegation: single

In `casper-agent`:
- When delegate sentinel detected in ReAct loop:
  1. Parse target agent name and message.
  2. Check depth (from delegate tool config).
  3. Create ephemeral conversation for child.
  4. `get_or_activate()` target, send, await with timeout.
  5. Return result as tool_result to parent.
- Depth exceeded → error, not infinite loop.

**Verify:** Parent delegates to child, child responds, parent gets result. Depth limit enforced.

---

### Task 5S — Delegation: parallel

In `casper-agent`:
- When LLM emits multiple delegate tool_use blocks in same turn:
  - Run all via `JoinSet`.
  - Collect results.
  - Append all tool_results.
  - Continue ReAct loop.

**Verify:** Two concurrent delegations, both complete, parent continues with both results.

---

### Task 5T — ask_user: suspend and resume

In `casper-agent`:
- When ask_user sentinel detected:
  1. Set `conversations.status = 'waiting'`.
  2. Return question + options to caller.
- On next message to same conversation:
  1. Set status back to `active`.
  2. Reactivate actor.
  3. Append human answer as user message.
  4. Resume ReAct loop.

**Verify:** Trigger ask_user, conversation waiting. Send answer, verify agent resumes.

---

### Task 5U — ask_user: MCP notification

In `casper-agent`:
- After setting conversation to waiting, read notification config from `ask_user` tool object.
- Call configured MCP server's tool with template-resolved params.
- Template variables: `{{agent.name}}`, `{{question}}`, `{{conversation.url}}`, `{{options}}`.
- If notification fails: log warning, don't block (the conversation is already waiting).

**Verify:** ask_user triggers, mock MCP receives notification with correct template variables.

---

## Phase 6 — Feedback & Training (2 tasks)

### Task 6A — Message feedback

- `POST /api/v1/feedback` — rating, correction, tag.
- `GET /api/v1/feedback` — filterable by agent, type, rating, date range.
- Validate: message exists, belongs to caller's tenant.

**Verify:** Rate, correct, tag. List with filters.

---

### Task 6B — Training data export

- `GET /api/v1/training/export` — JSONL and pairs formats.
- Quality tier filtering via `conversation_quality` view.
- Query params: agent, tier, outcome, date range.

**Verify:** Create conversations with feedback, export, verify structure and tier filtering.

---

## Phase 7 — Agent Export/Import (2 tasks)

### Task 7A — Agent export

- `GET /api/v1/agents/:name/export` — ZIP: `agent.yaml` + `instructions.md`.

**Verify:** Export, verify ZIP contents match agent config.

---

### Task 7B — Agent import

- `POST /api/v1/agents/import` — multipart ZIP.
- Create or update agent. Validate model_deployment exists.

**Verify:** Export → import → verify match. Import into different tenant.

---

## Phase 8 — MCP Client (3 tasks)

### Task 8A — MCP client and tool discovery

In `casper-mcp`:
- `McpClient`: connect to URL, authenticate with Bearer token.
- `discover_tools()` → list of tool definitions.
- `call_tool(name, input)` → result.
- SSE transport per MCP spec.

**Verify:** Integration test with mock MCP server: connect, discover, call.

---

### Task 8B — MCP tool registration in agent

In `casper-mcp` + `casper-agent`:
- On agent activation: connect to all MCP servers in `tools.mcp`.
- Discover tools, filter by `enabled_tools`.
- `McpTool` wrapper: adapts MCP tool to `Tool` trait.
- Register in `ToolDispatcher` alongside built-in tools.
- Secret resolution: `token_ref` from vault.

**Verify:** Agent with MCP config, tools appear in dispatcher, call executes via MCP.

---

### Task 8C — MCP elicitation forwarding

In `casper-mcp`:
- When an MCP server sends an elicitation request during a tool call:
  - Forward through the `ask_user` mechanism (conversation → waiting → notification).
  - Wait for operator response.
  - Send response back to MCP server.
- Timeout handling: if no response within tool timeout, cancel the MCP call.

**Verify:** MCP tool triggers elicitation, forwarded as ask_user, operator responds, MCP call completes.

---

## Phase 9 — CLI (2 tasks)

### Task 9A — CLI: auth and agent commands

In `casper-cli`:
- `clap` CLI, `reqwest` client.
- `casper auth login/status/logout`. Credentials in `~/.casper/credentials`.
- `casper run <agent> "<message>"` with `--conversation` and `--async`.
- `casper agent list/create/update/delete/export/import`.

**Verify:** Login, run agent, list agents.

---

### Task 9B — CLI: remaining commands

All other commands:
- `casper deploy list/create/test`
- `casper knowledge upload/list/search`
- `casper memory show/update/history`, `casper tenant-memory show/update`
- `casper secrets set/list/delete`, `casper keys create/list/update/revoke`
- `casper feedback submit`, `casper training export`
- `casper audit query`, `casper usage summary`
- `casper conversations list`, `casper snippets list`

**Verify:** Each command calls correct endpoint and displays results.

---

## Phase 10 — Frontend (12 tasks)

### Task 10A — Project setup, auth, and Playwright

React SPA: Vite + React 18 + TypeScript + Tailwind CSS v4.
- Auth store (Zustand): JWT storage, refresh interceptor.
- Login page (dev-login + SSO start).
- Axios client with auth headers.
- Types generated from API-v1.md.
- Playwright setup: `web/e2e/` directory, `playwright.config.ts` targeting `docker compose` test environment, `data-testid` convention.
- Playwright test helper: `createTestTenant()` → creates tenant + owner via API, returns credentials. Each test gets its own tenant.
- `web/e2e/auth.spec.ts`: dev login flow, verify redirect to dashboard, logout, invalid login rejected.

**Verify:** `npm run build`. Dev login works. `npx playwright test auth` passes.

---

### Task 10B — App shell and navigation

- Sidebar: Inference (Catalog, Deployments, Keys, Playground), Agents (Agents, Knowledge, Chat), Settings (Users, Secrets, Audit, Usage).
- Role-based section visibility.
- Top bar: user info, tenant, logout.
- `web/e2e/navigation.spec.ts`: verify sidebar sections per role (admin sees all, viewer sees subset), navigation between pages, top bar shows correct user.

**Verify:** Login, sidebar correct per role, navigation works. `npx playwright test navigation` passes.

---

### Task 10C — Model catalog and deployments

- Catalog: card grid, filter by provider/capabilities, quota status, deploy button.
- Deployments: table, create/edit slide-over, backend sequence, test button.
- `web/e2e/inference.spec.ts`: browse catalog (verify published models visible, unpublished not), create deployment, edit, test routing, delete. Verify quota status displayed correctly.

**Verify:** Browse catalog, create deployment, test routing. `npx playwright test inference` passes.

---

### Task 10D — API keys and playground

- API keys: table, create dialog, PATCH scopes, revoke.
- Playground: deployment selector, message input, streaming response, usage stats.
- `web/e2e/api-keys.spec.ts`: create key (verify shown once), list, update scopes, revoke.
- `web/e2e/playground.spec.ts`: select deployment, send message, verify response displayed, usage stats shown (wiremock LLM backend).

**Verify:** Create key, use in playground. `npx playwright test api-keys playground` passes.

---

### Task 10E — Agent builder: identity and model

- Agent name, display name, description fields.
- Model deployment selector.
- Save button (POST / PATCH, version increment).
- Agent list page with create/delete.
- `web/e2e/agent-crud.spec.ts`: create agent, verify in list. Edit name, verify version incremented. Delete, verify removed from list.

**Verify:** Create agent, save, verify in list. `npx playwright test agent-crud` passes.

---

### Task 10F — Agent builder: prompt stack

- Draggable block list.
- Add block menu (all 9 types).
- Per-type editors:
  - `text`: markdown editor
  - `environment`: checkbox list of fields
  - `agent_memory` / `tenant_memory`: toggle (presence = enabled)
  - `knowledge`: budget slider
  - `delegates`: agent selector with description + when
  - `datasource`: MCP/HTTP config with template variables
  - `snippet`: dropdown selector
  - `variable`: key + value fields
- Token budget display (live update).
- Drag to reorder.
- `web/e2e/prompt-stack.spec.ts`: add each block type, reorder via drag, verify order persisted after save. Edit text block content, save, reload, verify content. Remove block, verify removed.

**Verify:** Add all block types, reorder, verify prompt_stack JSONB correct on save. `npx playwright test prompt-stack` passes.

---

### Task 10G — Agent builder: tools configuration

- Built-in tools: toggle list with per-tool config editors.
  - `delegate`: timeout, max_depth inputs
  - `ask_user`: notification config (MCP server, tool, channel, message template)
  - `knowledge_search`: max_results, relevance_threshold
  - `update_memory`: max_document_tokens
  - `web_search`: max_results
  - `web_fetch`: timeout, max_response_bytes
- MCP servers: add/remove, URL, auth config, tool discovery with toggles.
- `web/e2e/tools-config.spec.ts`: add each built-in tool, configure settings, save, reload, verify settings persisted. Add MCP server, save, verify in agent tools JSONB.

**Verify:** Configure all built-in tools, add MCP server, save. `npx playwright test tools-config` passes.

---

### Task 10H — Agent builder: chat tab

- Send messages, see responses with tool call display.
- Status updates during agent processing.
- Feedback buttons (👍👎✏️🏷️) on assistant messages.
- Draft mode: uses current editor state without saving.
- Conversation selector: new / continue existing.
- `web/e2e/agent-chat.spec.ts`: send message, verify response displayed. Submit feedback (rating, correction, tag), verify persisted via API. Start new conversation, continue existing one. Test draft mode (change config, chat, verify unsaved config used).

**Verify:** Chat with agent, see tool calls, submit feedback. `npx playwright test agent-chat` passes.

---

### Task 10I — Agent builder: YAML and traces tabs

- YAML tab: live preview of agent export YAML (updates as you edit).
- Traces tab: audit log filtered by correlation_id. Shows LLM calls, tool calls, delegation, timing.
- `web/e2e/agent-tabs.spec.ts`: edit agent config, verify YAML tab updates live. Run agent, switch to traces tab, verify audit entries visible with correct correlation_id.

**Verify:** Edit agent, YAML preview updates. Run agent, traces show. `npx playwright test agent-tabs` passes.

---

### Task 10J — Knowledge, memory, snippets pages

- Knowledge: upload (drag-and-drop), list, search bar, delete.
- Agent memory: view document, edit (markdown editor), version history with diff view.
- Tenant memory: same as agent memory.
- Snippets: list, create/edit (markdown editor), token count, delete.
- `web/e2e/knowledge.spec.ts`: upload document, verify in list, search, verify results, delete.
- `web/e2e/memory.spec.ts`: view agent memory, edit, save, verify version history shows old + new. View diff between versions. Same for tenant memory.
- `web/e2e/snippets.spec.ts`: create snippet, verify token count, edit, delete.

**Verify:** Upload doc, search. Edit memory, view diff. Create snippet. `npx playwright test knowledge memory snippets` passes.

---

### Task 10K — Conversations page

- List: agent, title, status badge, outcome, date. Click to open.
- Detail: full message history with author attribution, tool call rendering.
- Outcome selector dropdown.
- Feedback buttons on assistant messages.
- `web/e2e/conversations.spec.ts`: create conversations via API, verify list shows them with correct status/outcome. Open conversation, verify messages displayed with authors. Set outcome, verify persisted. Submit feedback on a message, verify via API.

**Verify:** Browse, view messages, set outcome, submit feedback. `npx playwright test conversations` passes.

---

### Task 10L — Usage, audit, and admin pages

- Usage: summary cards, charts by source/model/agent, date picker.
- Audit: filterable table (action, actor, timestamp). Correlation ID links.
- Platform admin (platform:admin only):
  - Tenants: list, create, edit.
  - Models: list, create, edit (all capability fields).
  - Backends: list, create, assign to models.
  - Quotas: list, allocate per tenant/model.
  - SSO: configure per tenant.
  - Domains: manage per tenant.
- `web/e2e/usage.spec.ts`: generate usage via API, verify summary cards and charts show data.
- `web/e2e/audit.spec.ts`: trigger auditable actions, verify entries appear with correct filters.
- `web/e2e/admin.spec.ts`: full CRUD for tenants, models, backends, quotas, SSO, domains. Verify non-admin cannot access admin pages.

**Verify:** View usage, query audit. CRUD platform entities as admin. `npx playwright test usage audit admin` passes.

---

## Phase 11 — Tests & Polish (5 tasks)

### Task 11A — Inference integration tests

E2E: tenant → model → backend → quota → deployment → scoped API key → `/v1/chat/completions` with wiremock.
Test: three-part scope enforcement, quota exhaustion (429), backend failure/fallback, streaming, cache token tracking.

**Verify:** `cargo test -p casper-server --test inference` passes.

---

### Task 11B — Agent integration tests

E2E: agent → run → ReAct loop with wiremock → tool calls → response.
Test: delegation (single + parallel), ask_user (suspend + resume), memory update, knowledge search, max turns, datasource resolution.

**Verify:** `cargo test -p casper-server --test agent` passes.

---

### Task 11C — Auth integration tests

Test: dev login, JWT validation, three-part scope enforcement (two-part grants three-part, specific scopes, wildcard), API key auth, revocation, SSO flow (mocked callback).

**Verify:** `cargo test -p casper-server --test auth` passes.

---

### Task 11D — CI and Docker

- GitHub Actions: PostgreSQL service, `cargo build`, `cargo test`, `cargo clippy`.
- GitHub Actions: Playwright step — start backend via docker compose, run `npx playwright test`.
- `Dockerfile`: multi-stage for `casper-server`.
- `docker-compose.yml`: PostgreSQL + server + SearXNG for local dev.
- `docker-compose.test.yml`: same + wiremock LLM backend for Playwright tests.
- `.env.example`.

**Verify:** `docker compose up`, health check passes. `npx playwright test` passes against test compose.

---

### Task 11E — Documentation

- `README.md`: overview, quick start, architecture diagram.
- `config/casper-server.yaml`: complete documented example.
- `ARCHITECTURE.md`: crate structure, data flow, protocol reference.
- API examples with curl.

**Verify:** Follow README from scratch. Server starts, dev login, agent responds.

---

## Summary

| Phase | Tasks | Description |
|---|---|---|
| 0 | 4 | Project setup, migrations, server shell |
| 1 | 11 | Base types, scopes, JWT, DB, vault, observability, auth middleware |
| 2 | 7 | Tenants, SSO, OIDC, users, API keys, secrets |
| 3 | 9 | Model catalog, backends, quotas, deployments, routing, providers, inference endpoint |
| 4 | 5 | Knowledge, embeddings, agent memory, tenant memory, snippets |
| 5 | 20 | Agent CRUD, prompt assembler, conversations, tools, actors, ReAct, delegation, ask_user |
| 6 | 2 | Feedback, training export |
| 7 | 2 | Agent export/import |
| 8 | 3 | MCP client, tool integration, elicitation |
| 9 | 2 | CLI |
| 10 | 12 | Frontend (setup through admin pages) |
| 11 | 5 | Integration tests, CI, docs |
| **Total** | **82** | |

---

*End of milestones.*
