# Casper Platform — Technical Specification

**Version:** 1.0 | **Date:** April 2026 | **Status:** Draft
**Author:** Ventoo AG | **Classification:** Internal — Confidential

---

## 1. What Casper Is

Casper is a **multi-tenant AI agent platform** operated by Ventoo AG. It provides two product surfaces: an OpenAI-compatible LLM inference gateway and a stateful agent backend with tool execution, knowledge retrieval, memory, and delegation.

Tenants can use inference only (their developers call `/v1/chat/completions` from their apps) or the full agent system (configure agents with prompt stacks, connect MCP tools, let agents investigate and resolve issues). Both surfaces share the same routing, usage tracking, and auth infrastructure.

### 1.1 Key Design Decisions

**Ventoo-operated.** All inference backends are platform-managed. No multi-operator federation. Tenants access models via quotas allocated by Ventoo.

**MCP-first for external tools.** No sandbox/CLI infrastructure. Everything external — device relay, Azure management, ticketing systems, Slack — goes through MCP servers. Built-in tools handle only platform-internal operations (delegate, ask_user, knowledge_search, update_memory, web_search, web_fetch).

**Portal-first agent builder.** Agents are database-defined, configured through a Foundry-style portal UI with a composable prompt stack. Export/import provides portability.

**Memory is a living document.** Agent and tenant memory are versioned markdown documents maintained by agents and humans, not key-value stores.

**Three-part scopes.** `resource:identifier:action` format for fine-grained access control. A single mechanism covers both users and API keys.

**Single binary for V1.** One Rust server binary handles all routes (API, inference proxy, agent runtime). Service split is a future optimization.

### 1.2 What Casper Does Not Do (V1)

- No event bus or message queue — synchronous request/response with async as `tokio::spawn`
- No plan mode / workflow graphs — single ReAct loop only
- No timers or scheduled tasks
- No billing / invoicing — usage tracked, billing is V2
- No SAML — OIDC only
- No tenant-owned backends — Ventoo operates all inference
- No sandbox/container tool execution — MCP only for external tools

---

## 2. Architecture

### 2.1 System Topology

```
┌─────────────────┐     ┌──────────────────────────────┐
│   Portal (SPA)  │────▶│       casper-server           │
│   React/Vite    │     │       (Rust/Axum)             │
└─────────────────┘     │                              │
                        │  ┌─ Inference Layer ────────┐ │
┌─────────────────┐     │  │ Routing, retry/fallback, │ │
│   casper-cli    │────▶│  │ provider dispatch         │ │
│   (thin HTTP)   │     │  └──────────────────────────┘ │
└─────────────────┘     │                              │
                        │  ┌─ Agent Layer ────────────┐ │
┌─────────────────┐     │  │ Actors, ReAct loop,      │ │
│  External MCP   │◀───│  │ prompt stack, delegation  │ │
│  servers         │     │  └──────────────────────────┘ │
└─────────────────┘     └──────────────┬───────────────┘
                                       │
                        ┌──────────────┴───────────────┐
                        │  PostgreSQL 16 + pgvector     │
                        │  + filesystem (knowledge docs) │
                        └──────────────────────────────┘
```

### 2.2 Technology Stack

| Component | Technology |
|---|---|
| Language | Rust (edition 2024) |
| Web framework | Axum 0.8 |
| Async runtime | Tokio 1.x (full features) |
| Database driver | SQLx 0.8 (postgres, uuid, time, json, bigdecimal) |
| Database | PostgreSQL 16 + pgvector |
| JWT | Ed25519 (ed25519-dalek) |
| Encryption | AES-256-GCM + HKDF-SHA256 |
| Concurrent state | DashMap |
| HTTP client | reqwest |
| Frontend | React 18 + TypeScript + Vite + Tailwind CSS v4 |

### 2.3 Crate Structure

```
casper/
├── crates/
│   │  ── Shared ──
│   ├── casper-base/          # Types, JWT verification, RBAC, scopes, errors, SecretValue
│   ├── casper-db/            # PgPool, TenantDb, RLS wrapper
│   ├── casper-auth/          # JWT signing, revocation cache, OIDC
│   ├── casper-vault/         # Secret vault (AES-256-GCM, HKDF)
│   ├── casper-observe/       # Audit writer, usage tracking, Prometheus metrics
│   │
│   │  ── Inference ──
│   ├── casper-llm/       # Model catalog, backends, quotas, deployments, routing
│   ├── casper-proxy/         # LLM provider dispatch (Anthropic, OpenAI-compatible)
│   │
│   │  ── Agent ──
│   ├── casper-agent/         # Actors, ReAct loop, prompt stack, delegation, memory
│   ├── casper-knowledge/     # RAG: ingestion, chunking, embeddings, retrieval
│   └── casper-mcp/           # MCP client, tool discovery, elicitation forwarding
│
├── casper-server/            # Main binary: Axum, all routes, wiring
├── casper-cli/               # CLI binary: thin HTTP client
├── web/                      # React SPA
├── migrations/               # SQL migrations
└── config/                   # Server config (YAML)
```

**Dependency graph:**

```
                casper-base         ← types, no DB
              /     |     \
     casper-db  casper-auth  casper-vault   ← DB access, crypto
          |         |            |
     casper-observe  casper-llm         ← business logic
          |              |
     casper-proxy   casper-knowledge        ← external calls
          |              |
     casper-mcp     casper-agent            ← agent runtime
               \    /
            casper-server                   ← wires everything
```

---

## 3. Authentication & Authorization

### 3.1 Auth Methods

**User JWTs** — for humans (portal, CLI). Ed25519-signed. Short-lived access tokens (15 min) + rotating refresh tokens. Issued via OIDC (production) or dev-login (development).

**API keys** — for machines (webhooks, CI/CD, external apps). SHA-256 hashed at rest. Plaintext returned once at creation. Database lookup on every request. Single machine-to-machine mechanism for both inference and agent operations.

### 3.2 Tenant-Specific OIDC

Each tenant configures their identity provider (Azure AD, Google Workspace, Okta). Discovery by email domain:

```
User enters email → extract domain → lookup email_domains → find tenant
→ lookup sso_providers → redirect to tenant's IdP → callback → verify → issue JWT
```

Platform admins configure SSO during customer onboarding. Tenants cannot change their SSO config.

### 3.3 Three-Part Scopes

Scopes follow `resource[:identifier]:action` pattern:

```
agents:run                    → run ANY agent (two-part = all)
agents:triage:run             → run ONLY triage agent (three-part = specific)
agents:*:run                  → run any agent (explicit wildcard)
inference:sonnet-fast:call    → call ONLY this deployment
inference:*:call              → call any deployment
admin:*                       → everything
```

A two-part scope implicitly grants all identifiers. This applies to both user roles and API keys — one mechanism for everything.

### 3.4 JWT Structure

```json
{
  "sub": "user:andi@ventoo.ch",
  "tid": "a1b2c3d4-...",
  "role": "admin",
  "scopes": ["admin:*"],
  "exp": 1743292800,
  "iss": "casper",
  "jti": "unique-token-id"
}
```

### 3.5 Token Revocation

JWTs are stateless — once signed, valid until expiry. The `token_revocations` table + in-memory cache enables early invalidation (logout, key compromise, offboarding). Checked on every request.

---

## 4. Inference Layer

The inference layer is a complete LLM gateway. A tenant that only needs inference never touches the agent features.

### 4.1 Model Catalog

Platform-managed. All models visible to all tenants (when `published = true`). Models have structured capability columns (`cap_chat`, `cap_embedding`, `cap_thinking`, `cap_vision`, `cap_tool_use`, `cap_json_output`, `cap_audio_in`, `cap_audio_out`, `cap_image_gen`) and numeric capabilities (`context_window`, `max_output_tokens`, `embedding_dimensions`).

Embedding models go through the same catalog, routing, and deployment mechanism as chat models. No special casing.

### 4.2 Quotas

Per-tenant, per-model allocation. Default is 0 — model visible in catalog but not deployable. Includes rate limits, token limits, cache limits, and pricing overrides.

### 4.3 Deployments

Tenant-scoped routing configurations. A deployment maps a slug to a model with specific backend sequence, retry/fallback behavior, default parameters, and rate limits.

```
API call: model="sonnet-fast"
  → resolve: model_deployments(tenant_id, slug="sonnet-fast")
  → check quota
  → merge default_params
  → route through backend_sequence with retry/fallback
  → record usage (including cache tokens)
```

### 4.4 Routing & Retry

For each backend in the sequence:
1. Retry up to `retry_attempts` with exponential backoff (`retry_backoff_ms * 2^attempt`)
2. On final failure with `fallback_enabled`: try next backend
3. All exhausted: 503

### 4.5 Provider Adapters

Two adapters: **Anthropic** (Messages API) and **OpenAI-compatible** (Chat Completions API — covers OpenAI, Azure OpenAI, Mistral, Ollama, vLLM, LiteLLM).

### 4.6 Pricing & Usage

Dimensionless pricing (implicitly CHF). Four cost dimensions: input tokens, output tokens, cache read tokens, cache write tokens. Cost resolution: quota override > model default. Usage events track all four token types plus backend, deployment, source, and correlation ID.

---

## 5. Agent Runtime

### 5.1 Actors

Every conversation is an independent actor: `(tenant_id, agent_name, conversation_id)`.

**Registry:** In-memory `DashMap<ActorKey, ActorHandle>`. Not database-backed.

**Lifecycle:** First request spawns a tokio task with a bounded mailbox. Subsequent requests find the handle and send. Sequential processing — no concurrent mutations. Idle reaper dehydrates after configurable timeout. Next request reactivates.

**Multiple users:** Any authenticated user with the right scope can send messages to an existing conversation. Messages carry `author` for attribution.

### 5.2 ReAct Loop

Standard tool-using agent loop:

```
1. Assemble prompt (prompt stack + history + current message)
2. Call LLM (via inference layer — same routing/retry)
3. If response contains tool_use blocks:
   a. Partition: delegate calls run in parallel (JoinSet), others sequential
   b. Execute each tool via the appropriate mechanism:
      - Built-in: in-process function call
      - MCP: call MCP server
   c. Append tool results to conversation
   d. Loop (back to step 1)
4. If end_turn: append assistant message, return response
5. Max turns check: stop if exceeded
```

### 5.3 Delegation

Parallel delegation when the LLM emits multiple `delegate` tool calls simultaneously:

```
triage agent calls:
  delegate({ agent: "devops-agent", message: "Get pipeline logs" })
  delegate({ agent: "infra-agent", message: "Check VM health" })

Both run in parallel via JoinSet. Each creates an ephemeral conversation
for the child agent, sends to its mailbox, awaits response.
```

Depth tracked per request chain. Timeout and max depth configured on the `delegate` tool.

### 5.4 ask_user

Agent suspends execution and asks the human a question. The conversation status becomes `waiting`. Notification sent via the configured MCP server (e.g. Slack with action buttons). The operator responds via:

- Slack buttons → MCP server calls Casper API
- CLI → `casper run triage "Yes" --conversation conv-003`
- Portal → direct API call
- Any HTTP client → `POST /api/v1/agents/triage/run` with `conversation_id`

No chat WebSocket required. The API is the universal response mechanism.

---

## 6. Prompt Stack

Every agent has an ordered `prompt_stack` — an array of typed blocks assembled into the system prompt at runtime. The order determines LLM attention priority (top = highest).

### 6.1 Block Types

| Type | Purpose | Budget |
|---|---|---|
| `text` | Static instructions (markdown) | Fixed (always included) |
| `environment` | Runtime context (datetime, tenant, source, invocation metadata) | Fixed |
| `agent_memory` | Agent's versioned memory document | Fixed (agent manages length) |
| `tenant_memory` | Shared tenant memory document (read-only for agents) | Fixed |
| `knowledge` | RAG retrieval from knowledge base | Capped (budget_tokens) |
| `delegates` | Available sub-agents with descriptions and when-to-use guidance | Fixed |
| `datasource` | External data fetched at assembly time via MCP or HTTP | Capped (budget_tokens) |
| `snippet` | Reusable block from tenant's snippet library | Fixed |
| `variable` | Custom key-value pair | Fixed |

All blocks are optional. The user composes the stack in the portal's drag-and-drop builder.

### 6.2 Budget Allocation

```
Available = model.context_window - output_reserved
1. Static blocks (text, environment, variable, snippet, delegates): always included
2. Datasource blocks: fetched at assembly time, budget-capped
3. Knowledge: fill up to budget with highest-ranked chunks
4. Agent memory: full document
5. Tenant memory: full document
6. Conversation history: gets remaining budget (most recent turns first)
7. Current message: always included
```

### 6.3 Template Variables

Text and datasource blocks support template variables:

- `{{tenant.display_name}}`, `{{tenant.slug}}`
- `{{agent.name}}`, `{{agent.display_name}}`
- `{{metadata.*}}` — resolved from the `/run` call's metadata field

Datasource blocks use metadata variables to parameterize MCP tool calls or HTTP requests at assembly time.

### 6.4 Datasource Blocks

Fetch external data before the first LLM call. The result is injected into the prompt — the agent starts with context already loaded.

```json
{
  "type": "datasource",
  "label": "Customer Info",
  "source": {
    "type": "mcp",
    "server": "Ventoo Apps",
    "tool": "get_customer",
    "params": { "id": "{{metadata.customer_id}}" }
  },
  "budget_tokens": 500,
  "on_missing": "skip"
}
```

`on_missing`: `"skip"` (silently omit if metadata variable missing) or `"fail"` (reject the request).

---

## 7. Tools

### 7.1 Built-In Tools

Six tools that interact with the platform. Each configured as an object with tool-specific settings:

| Tool | Purpose | Key configuration |
|---|---|---|
| `delegate` | Call another agent, await result | `timeout_secs`, `max_depth` |
| `ask_user` | Suspend and ask human a question | `notification` (MCP server, channel, template) |
| `knowledge_search` | Search tenant knowledge base (RAG) | `max_results`, `relevance_threshold` |
| `update_memory` | Rewrite agent's memory document | `max_document_tokens` |
| `web_search` | Search the web (SearXNG, server-level config) | `max_results` |
| `web_fetch` | Fetch a URL | `timeout_secs`, `max_response_bytes` |

### 7.2 MCP Tools

All external tool execution goes through MCP servers. Configured per-agent:

```json
{
  "mcp": [{
    "name": "Ventoo Relay",
    "url": "https://mcp.ventoo.ai/relay/mcp",
    "auth": { "type": "bearer", "token_ref": "secret:relay_service_token" },
    "enabled_tools": ["connect_device", "relay_exec", "relay_screenshot", "disconnect_device"]
  }]
}
```

MCP auth tokens stored in the tenant's secret vault. Tools discovered dynamically via `tools/list`. The agent builder UI shows discovered tools and lets users toggle which ones the agent can use.

MCP elicitation (for operator approval) is forwarded through the `ask_user` notification mechanism.

### 7.3 Tool Resolution

The tool dispatcher merges built-in tools and MCP tools into a unified list for the LLM. The LLM doesn't know or care whether a tool is built-in or MCP.

---

## 8. Memory

### 8.1 Agent Memory

A single markdown document per agent per tenant. Always loaded into the prompt via the `agent_memory` block. The agent reads it, works with it, and calls `update_memory` to rewrite it — adding observations, removing stale info, reorganizing.

Example:

```markdown
# Triage Agent — Memory

## Customers
### Acme Corp
- M365 E5, Azure AD hybrid join
- VPN gateway times out under load — restart gateway service
- Maintenance window: Tuesdays 2-4 AM UTC

## Known Issues
- Outlook 16.0.18324.20194 crashes with Grammarly add-in (KB5042567)

## Patterns
- P1 database issues: check DTU first, then recent deployments
```

Every update creates a version in `agent_memory_history`. Humans can edit via the portal or API. Full version history with diffs.

### 8.2 Tenant Memory

Same model — one markdown document per tenant. Read by all agents, updated by humans only. Contains facts that every agent should know: customer details, policies, procedures.

---

## 9. Knowledge Base

### 9.1 Storage

Source files on filesystem: `data/knowledge/{tenant_id}/{document_id}.{ext}`. Metadata in `documents` table. Chunks + embeddings in `document_chunks` table.

### 9.2 Ingestion Pipeline

Upload → extract text (PDF, Markdown, plain text, HTML, DOCX) → chunk (~500 tokens with overlap) → generate embeddings via inference layer (same deployment mechanism) → store chunks with vectors.

### 9.3 Retrieval

At prompt assembly time: embed the user's query → cosine similarity search on `document_chunks` → top-k chunks within the knowledge block's token budget → inject into system prompt.

---

## 10. Conversations

### 10.1 Model

Each conversation belongs to one agent and one tenant. Conversations are the actor instance key: `(tenant_id, agent_name, conversation_id)`. Multiple users can participate — messages carry an `author` field.

### 10.2 Statuses

- `active` — normal conversation
- `waiting` — agent called `ask_user`, awaiting human response
- `completed` — conversation ended

### 10.3 Outcomes

Set by operators: `resolved`, `unresolved`, `escalated`, `duplicate`. Used for quality tracking and training data filtering.

### 10.4 Messages

Every step of the ReAct loop is a message: user turns, assistant responses, tool_use blocks, tool_result blocks. Full content preserved for: conversation history in subsequent turns, training data export, audit trail, traces view in the portal.

---

## 11. Feedback & Training Data

### 11.1 Feedback Types

- **Rating** (+1/-1) — thumbs up/down on assistant messages
- **Correction** — operator provides a better response (creates preference pair for DPO/RLHF)
- **Tag** — categorize issues: hallucination, wrong_tool, incomplete, wrong_answer, too_verbose, too_terse, good_delegation, good_tool_use, good_explanation

### 11.2 Quality Tiers

| Tier | Criteria | Use for |
|---|---|---|
| Gold | Has operator correction | DPO / RLHF |
| Silver | Avg rating ≥ 4, resolved, tool success > 80%, no hallucination | SFT fine-tuning |
| Bronze | Resolved, no negative ratings | Pre-training corpus |
| Excluded | Unresolved, negative ratings, hallucination, > 15 turns | Never train on |

### 11.3 Export

JSONL format (conversation turns with full message history and quality metadata) or preference pairs format (chosen/rejected for DPO).

---

## 12. Observability

### 12.1 Audit Log

Append-only, per-tenant. Every significant action recorded: auth events, agent runs, tool calls, memory updates, config changes. Buffered channel — writes never block the caller.

### 12.2 Usage Tracking

Unified `usage_events` table for all LLM calls: inference API, agent internal calls, playground. Tracks input/output/cache_read/cache_write tokens, model, deployment, agent, backend, correlation ID.

### 12.3 Metrics

Prometheus at `/metrics`. Counters and histograms for: LLM calls, tool executions, actor activations/dehydrations, request durations.

---

## 13. Server Configuration

Platform-level settings in `casper-server.yaml`:

```yaml
listen:
  port: 3000
  cors_origins: ["https://casper.ventoo.ai"]

database:
  url: postgres://casper:***@localhost:5432/casper
  main_pool_size: 30
  analytics_pool_size: 10

auth:
  signing_key_file: /run/secrets/casper_signing_key
  master_key_file: /run/secrets/casper_master_key
  dev_auth: false
  admin_email: admin@ventoo.ch

web_search:
  base_url: https://search.ventoo.ai
  format: json
  max_results: 10

web_fetch:
  timeout_secs: 30
  max_response_bytes: 1048576

knowledge:
  storage_path: ./data/knowledge
  embedding_deployment: embeddings-default

actors:
  idle_timeout_secs: 300
  max_concurrent_per_tenant: 50
```

---

## 14. Agent Export/Import Format

Agents are portable. Export produces a ZIP:

```
triage/
├── agent.yaml        # Full config: model, tools, prompt_stack, config
└── instructions.md   # Text blocks extracted for readability
```

Import accepts the same ZIP. Creates or updates the agent in the target tenant. Validates that referenced model deployments exist (or returns error listing what's needed).

---

## 15. Portal

Foundry-style agent builder with split view:

**Left panel (configuration):**
- Agent identity (name, display name, description)
- Model deployment selector
- Prompt stack: draggable block list with per-type editors
- Tools: built-in tool configuration + MCP server management with tool discovery
- Token budget display (updates live)

**Right panel (playground):**
- Chat: send messages, see responses with tool calls and status updates. Feedback buttons (👍👎✏️🏷️) on assistant messages. Draft testing (uses current editor state without saving).
- YAML: live export preview
- Traces: audit log filtered by correlation_id

**Other pages:**
- Model catalog (browse, filter, deploy)
- Deployments (routing config, test, manage)
- API keys (create, scope, revoke)
- Knowledge (upload, search, manage)
- Memory (agent + tenant, version history, edit)
- Snippets (create, edit, reuse across agents)
- Conversations (browse, view messages, set outcome)
- Usage (summary, charts, by source/model/agent)
- Audit (filterable log)
- Platform admin (tenants, models, backends, quotas, SSO, domains)

---

## 16. Future Considerations

- **ReBAC** — relationship-based access control for per-agent/per-knowledge permissions
- **Tenant-owned backends** — tenants bring their own API keys
- **Plan mode** — LLM creates a WorkflowGraph, engine walks it node by node
- **Timers** — scheduled agent tasks
- **Event bus** — async ingestion for high-volume webhook scenarios
- **Config versioning / canary** — gradual agent config rollout
- **Guardrails** — input/output validation pipeline
- **Billing** — forex, invoicing, payment integration
- **Chat WebSocket** — real-time streaming in the portal (V1 works without it via API)

---

*End of specification.*
