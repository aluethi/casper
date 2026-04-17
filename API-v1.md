# Casper Platform — API Surface v1

**Version:** 1.0 | **Date:** April 2026 | **Status:** Draft
**Classification:** Internal — Confidential

---

## 1. Overview

Casper exposes two product surfaces through a single API server:

**Inference API** — OpenAI-compatible LLM gateway. Developers authenticate with API keys, create model deployments with tenant-specific routing/retry/fallback, and call `/v1/chat/completions`. Stateless request/response inference with usage tracking. Quota-based access control. Embedding models treated identically to chat models.

**Agent API** — Stateful agent backend. Tenants create and configure agents through a portal-first builder (composable prompt stack, tools, knowledge, model). Agents run a ReAct loop with six built-in tools (delegate, ask_user, knowledge_search, update_memory, web_search, web_fetch) plus MCP server integrations. All external tool execution goes through MCP — no sandbox/CLI infrastructure. Agent definitions are portable via export/import.

All agent interactions are captured for quality improvement and future model fine-tuning.

### 1.1 Base URLs

```
https://casper.ventoo.ai/v1/...           # Inference API (OpenAI-compatible)
https://casper.ventoo.ai/api/v1/...       # Management + Agent API
https://casper.ventoo.ai/auth/...         # Authentication
https://casper.ventoo.ai/ws/...           # WebSocket (chat, agent backends)
https://casper.ventoo.ai/health           # Health check
https://casper.ventoo.ai/metrics          # Prometheus metrics
```

### 1.2 Authentication

| Method | Header | Use case |
|---|---|---|
| API key | `Authorization: Bearer csk-...` | Inference API, webhooks, CI/CD, any machine-to-machine |
| JWT (user session) | `Authorization: Bearer csp_eyJ...` | Portal, CLI (human users) |

API keys are the single machine-to-machine auth mechanism. Ed25519-signed JWTs for user sessions.

### 1.3 Authorization

Four roles, strictly ordered: `viewer < operator < admin < owner`.

Scopes follow `resource[:identifier]:action` pattern:

```
Two-part (all resources):     agents:run          → run any agent
Three-part (specific):        agents:triage:run   → run only triage
Wildcard:                     agents:*:run        → run any agent (explicit)
                              inference:*:call    → call any deployment
                              admin:*             → everything
```

A two-part scope implicitly grants all identifiers: `agents:run` = `agents:*:run`.

**Standard scopes:**

| Pattern | Grants |
|---|---|
| `inference[:deployment]:call` | Make inference API calls |
| `agents[:agent]:run` | Send messages to agents |
| `agents:manage` | Create/update/delete agents |
| `knowledge:read` | Search knowledge base |
| `knowledge:write` | Upload/delete documents |
| `memory:read` | Read agent and tenant memory |
| `memory:write` | Update agent and tenant memory |
| `secrets:read` | List secret keys |
| `secrets:write` | Set/delete secrets |
| `feedback:write` | Submit feedback on agent messages |
| `training:export` | Export training data |
| `audit:read` | Query audit log |
| `usage:read` | Query usage data |
| `users:manage` | Invite/update/remove users |
| `keys:manage` | Create/revoke API keys |
| `config:manage` | Manage model deployments |
| `admin:*` | All tenant operations |
| `platform:admin` | Platform-level operations (Ventoo staff only) |

### 1.4 Common Patterns

**Pagination:**
```
GET /api/v1/resource?page=1&per_page=50
→ { "data": [...], "pagination": { "page": 1, "per_page": 50, "total": 234 } }
```

**Error responses:**
```json
{ "error": "forbidden", "message": "Token lacks scope agents:triage:run" }
```

HTTP status codes: 400, 401, 403, 404, 409, 429, 500, 502, 504.

---

## 2. Authentication Routes

```
POST   /auth/login              # Dev login (dev mode only)
POST   /auth/sso/start          # Start tenant-specific OIDC flow
GET    /auth/sso/callback       # OIDC callback
POST   /auth/refresh            # Refresh access token
POST   /auth/logout             # Revoke refresh token
GET    /auth/status             # Current user info
```

### POST /auth/sso/start

Tenant-specific OIDC. Discovers identity provider from email domain.

```json
{ "email": "jane@acme.com" }
```

Flow: extract domain `acme.com` → lookup `email_domains` → find tenant → lookup `sso_providers` → build OIDC URL with PKCE → return redirect.

---

## 3. Inference API (OpenAI-Compatible)

### POST /v1/chat/completions

**Auth:** API key (`csk-`) or JWT. Scope: `inference[:deployment]:call`.

```json
{
  "model": "sonnet-fast",
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "Hello!" }
  ],
  "max_tokens": 1024,
  "stream": false
}
```

**Resolution:**
1. Authenticate → extract `tenant_id` + scopes
2. Check scope: does caller have `inference:sonnet-fast:call` or `inference:*:call` or `inference:call`?
3. Lookup `model_deployments(tenant_id, slug = "sonnet-fast")`
4. Check `model_quotas` — reject if quota is 0 or exhausted
5. Merge `deployment.default_params` under request body (request params win)
6. Route through backend sequence with retry/fallback
7. Record usage event (including cache tokens if applicable)

### GET /v1/models

Returns deployments the caller's scopes grant access to. If scopes include `inference:call` (two-part), returns all. If `inference:sonnet-fast:call` (three-part), returns only that one.

---

## 4. Platform Administration

**Auth:** JWT with `platform:admin` scope.

### 4.1 Tenants

```
POST   /api/v1/tenants
GET    /api/v1/tenants
GET    /api/v1/tenants/:id
PATCH  /api/v1/tenants/:id
```

### 4.2 Tenant SSO

```
POST   /api/v1/tenants/:id/sso
GET    /api/v1/tenants/:id/sso
PATCH  /api/v1/tenants/:id/sso
DELETE /api/v1/tenants/:id/sso
```

### 4.3 Tenant Email Domains

```
POST   /api/v1/tenants/:id/domains
GET    /api/v1/tenants/:id/domains
DELETE /api/v1/tenants/:id/domains/:domain
```

### 4.4 Model Catalog

```
POST   /api/v1/models
GET    /api/v1/models                   # All models (including unpublished)
GET    /api/v1/models/:id
PATCH  /api/v1/models/:id
```

```json
{
  "name": "claude-sonnet-4",
  "display_name": "Claude Sonnet 4",
  "provider": "anthropic",
  "context_window": 200000,
  "max_output_tokens": 8192,
  "embedding_dimensions": null,
  "cap_chat": true,
  "cap_embedding": false,
  "cap_thinking": true,
  "cap_vision": true,
  "cap_tool_use": true,
  "cap_json_output": true,
  "cap_audio_in": false,
  "cap_audio_out": false,
  "cap_image_gen": false,
  "cost_per_1k_input": 0.003,
  "cost_per_1k_output": 0.015,
  "cost_per_1k_cache_read": 0.0003,
  "cost_per_1k_cache_write": 0.00375,
  "published": false
}
```

### 4.5 Backend Assignment

```
POST   /api/v1/models/:id/backends
GET    /api/v1/models/:id/backends
DELETE /api/v1/models/:id/backends/:backend_id
```

### 4.6 Platform Backends

```
POST   /api/v1/backends
GET    /api/v1/backends
GET    /api/v1/backends/:id
PATCH  /api/v1/backends/:id
```

### 4.7 Model Quotas

```
POST   /api/v1/tenants/:id/quotas
GET    /api/v1/tenants/:id/quotas
PATCH  /api/v1/tenants/:id/quotas/:model_id
DELETE /api/v1/tenants/:id/quotas/:model_id
```

```json
{
  "model_id": "...",
  "requests_per_minute": 100,
  "tokens_per_day": 1000000,
  "cache_tokens_per_day": 5000000,
  "cost_per_1k_input": 0.004,
  "cost_per_1k_output": 0.018,
  "cost_per_1k_cache_read": 0.0004,
  "cost_per_1k_cache_write": 0.005
}
```

---

## 5. Tenant Management

### 5.1 Model Catalog (Tenant View)

```
GET    /api/v1/catalog                  # Published models only, with quota status
```

### 5.2 Model Deployments

```
POST   /api/v1/deployments
GET    /api/v1/deployments
GET    /api/v1/deployments/:id
PATCH  /api/v1/deployments/:id
DELETE /api/v1/deployments/:id
POST   /api/v1/deployments/:id/test
```

**Auth:** `config:manage` for writes, `inference:call` for reads.

### 5.3 API Keys

```
POST   /api/v1/api-keys
GET    /api/v1/api-keys
GET    /api/v1/api-keys/:id
PATCH  /api/v1/api-keys/:id             # Update name, scopes (key string unchanged)
DELETE /api/v1/api-keys/:id
```

**Auth:** `keys:manage`. Scopes use the three-part format for resource restriction:

```json
{
  "name": "chatbot-key",
  "scopes": ["inference:sonnet-fast:call", "inference:gpt4o:call"]
}
```

```json
{
  "name": "webhook-triage",
  "scopes": ["agents:triage:run"]
}
```

### 5.4 Users

```
POST   /api/v1/users
GET    /api/v1/users
GET    /api/v1/users/:id
PATCH  /api/v1/users/:id
DELETE /api/v1/users/:id
```

### 5.5 Secrets Vault

```
POST   /api/v1/secrets
GET    /api/v1/secrets                  # Keys only, never values
DELETE /api/v1/secrets/:key
```

Stores outbound credentials for MCP server authentication.

---

## 6. Agent API

### 6.1 Agent CRUD

**Auth:** `agents:manage` for create/update/delete, `agents:run` for read.

```
POST   /api/v1/agents
GET    /api/v1/agents
GET    /api/v1/agents/:name
PATCH  /api/v1/agents/:name
DELETE /api/v1/agents/:name
GET    /api/v1/agents/:name/export
POST   /api/v1/agents/import
```

**POST /api/v1/agents**

```json
{
  "name": "triage",
  "display_name": "Triage Agent",
  "description": "Investigates and triages incoming support requests",
  "model_deployment": "sonnet-fast",
  "prompt_stack": [
    { "type": "text", "label": "Instructions", "content": "You are the triage agent..." },
    { "type": "environment", "label": "Environment", "include": ["datetime", "tenant_name", "invocation_source"] },
    { "type": "text", "label": "Input Routing", "content": "How you respond depends on how you were invoked..." },
    { "type": "tenant_memory", "label": "Shared Knowledge" },
    { "type": "agent_memory", "label": "Agent Memory" },
    { "type": "knowledge", "label": "Knowledge Base", "budget_tokens": 2000 },
    { "type": "delegates", "label": "Available Agents", "agents": [
        { "name": "devops-agent", "description": "CI/CD and deployments", "when": "Build failures or pipeline issues" },
        { "name": "m365-agent", "description": "Microsoft 365", "when": "Mail flow, licenses, Teams/SharePoint" }
    ]},
    { "type": "datasource", "label": "Customer Info", "source": {
        "type": "mcp", "server": "Ventoo Apps", "tool": "get_customer",
        "params": { "id": "{{metadata.customer_id}}" }
    }, "budget_tokens": 500, "on_missing": "skip" },
    { "type": "snippet", "label": "Response Style", "snippet_id": "..." },
    { "type": "variable", "label": "Escalation Contact", "key": "escalation_email", "value": "oncall@acme.com" }
  ],
  "tools": {
    "builtin": [
      { "name": "delegate", "timeout_secs": 300, "max_depth": 3 },
      { "name": "ask_user", "notification": {
          "mcp_server": "Slack",
          "tool": "send_message",
          "params": {
            "channel": "#incidents",
            "text": "Agent {{agent.name}} needs input: {{question}}\n{{conversation.url}}"
          }
      }},
      { "name": "knowledge_search", "max_results": 5, "relevance_threshold": 0.7 },
      { "name": "update_memory", "max_document_tokens": 4000 },
      { "name": "web_search", "max_results": 10 },
      { "name": "web_fetch", "timeout_secs": 30, "max_response_bytes": 1048576 }
    ],
    "mcp": [
      {
        "name": "Ventoo Relay",
        "url": "https://mcp.ventoo.ai/relay/mcp",
        "auth": { "type": "bearer", "token_ref": "secret:relay_service_token" },
        "enabled_tools": ["connect_device", "relay_exec", "relay_screenshot", "disconnect_device"]
      }
    ]
  },
  "config": {
    "max_turns": 20
  }
}
```

**Prompt stack block types:**

| Type | Purpose |
|---|---|
| `text` | Static instructions (markdown) |
| `environment` | Runtime context (datetime, tenant, source) |
| `agent_memory` | Agent's versioned memory document |
| `tenant_memory` | Shared tenant memory document (read-only for agents) |
| `knowledge` | RAG retrieval from knowledge base |
| `delegates` | Available sub-agents with descriptions |
| `datasource` | External data fetched at assembly time via MCP or HTTP |
| `snippet` | Reusable block from snippets library |
| `variable` | Custom key-value pair |

**Built-in tools (all configured as objects):**

| Tool | Configuration |
|---|---|
| `delegate` | `timeout_secs`, `max_depth` |
| `ask_user` | `notification` (MCP server, tool, params with template variables) |
| `knowledge_search` | `max_results`, `relevance_threshold` |
| `update_memory` | `max_document_tokens` |
| `web_search` | `max_results` (provider is server-level SearXNG config) |
| `web_fetch` | `timeout_secs`, `max_response_bytes` |

All external tool execution goes through MCP servers — no CLI/sandbox infrastructure.

### 6.1.1 Export / Import

**GET /api/v1/agents/:name/export** — ZIP with `agent.yaml` + `instructions.md`.
**POST /api/v1/agents/import** — accepts ZIP, creates or updates agent.

### 6.2 Agent Interaction

**Auth:** `agents[:agent]:run` scope.

```
POST   /api/v1/agents/:name/run
GET    /api/v1/agents/:name/tasks/:id   # Poll async result
```

```json
{
  "message": "Why is the prod database slow?",
  "conversation_id": null,
  "source": "user",
  "metadata": { "ticket_id": "4521", "customer_id": "acme-corp" },
  "async": false,
  "draft": null
}
```

**Response:**

```json
{
  "conversation_id": "...",
  "message": { "id": "...", "role": "assistant", "content": "The database CPU is at 95%..." },
  "usage": { "input_tokens": 5400, "output_tokens": 1140, "cache_read_tokens": 2000, "cache_write_tokens": 500, "llm_calls": 3, "tool_calls": 1, "delegations": 1, "duration_ms": 8700 },
  "correlation_id": "..."
}
```

**`ask_user` flow:** Agent calls `ask_user` → conversation status becomes `waiting` → notification sent via configured MCP server (e.g. Slack with action buttons) → operator responds via API (`POST /api/v1/agents/:name/run` with existing `conversation_id`), Slack buttons, or CLI → agent resumes. No chat UI required.

### 6.3 Conversations

```
GET    /api/v1/conversations
GET    /api/v1/conversations/:id
DELETE /api/v1/conversations/:id
PATCH  /api/v1/conversations/:id/outcome
```

Filterable by agent, status, outcome, participant. Multiple users can participate. Messages carry `author` field.

**Status:** `active`, `waiting`, `completed`.
**Outcome:** `resolved`, `unresolved`, `escalated`, `duplicate`.

### 6.4 Knowledge Base

```
POST   /api/v1/knowledge               # Multipart upload
GET    /api/v1/knowledge
GET    /api/v1/knowledge/:id
DELETE /api/v1/knowledge/:id
POST   /api/v1/knowledge/search
```

Source files stored on filesystem at `data/knowledge/{tenant_id}/{document_id}.{ext}`. Metadata, chunks, and embeddings in database.

### 6.5 Agent Memory

```
GET    /api/v1/agents/:name/memory
PUT    /api/v1/agents/:name/memory
GET    /api/v1/agents/:name/memory/history
GET    /api/v1/agents/:name/memory/history/:version
```

Single versioned markdown document per agent. Updated by agent (`update_memory` tool) or human (API/portal). Full version history.

### 6.6 Tenant Memory

```
GET    /api/v1/tenant-memory
PUT    /api/v1/tenant-memory
GET    /api/v1/tenant-memory/history
GET    /api/v1/tenant-memory/history/:version
```

Shared document across all agents. Updated by humans only.

### 6.7 Snippets

```
POST   /api/v1/snippets
GET    /api/v1/snippets
GET    /api/v1/snippets/:id
PATCH  /api/v1/snippets/:id
DELETE /api/v1/snippets/:id
```

### 6.8 Tools

```
GET    /api/v1/tools                    # List built-in tools and their schemas
```

Read-only. Built-in tools are platform-defined. MCP tools are discovered from configured servers.

---

## 7. Feedback & Training Data

### 7.1 Message Feedback

```
POST   /api/v1/feedback
GET    /api/v1/feedback
```

**Auth:** `feedback:write`. Types: `rating` (+1/-1), `correction` (better response), `tag` (hallucination, wrong_tool, etc.).

### 7.2 Training Data Export

```
GET    /api/v1/training/export
```

**Auth:** `training:export`. Formats: `jsonl` (SFT), `pairs` (DPO). Quality tiers: gold (corrections), silver (high-rated resolved), bronze (resolved).

---

## 8. Observability

```
GET    /api/v1/audit                    # Audit log query
GET    /api/v1/usage                    # Usage summary
GET    /api/v1/usage/events             # Detailed usage events
GET    /health                          # Readiness check
GET    /metrics                         # Prometheus metrics
```

---

## 9. WebSocket Endpoints

### 9.1 Chat (optional for V1)

```
GET    /ws/chat
```

JWT-authenticated. Streams agent responses for portal chat UI. Not required for core flow — API + Slack notifications cover all use cases.

### 9.2 Agent Backend (Ventoo Self-Hosted GPU)

```
GET    /agent/connect
```

Agent key (`csa-`) authenticated. Ventoo-operated GPU servers connect via WebSocket to serve inference requests. See `AGENT-BACKENDS-v1.md` for full specification. This is an enhancement — not part of V1 core.

---

## 10. CLI Mapping

```
casper auth login               → POST /auth/login or /auth/sso/start
casper auth status              → GET /auth/status
casper auth logout              → POST /auth/logout

casper run <agent> "<message>"  → POST /api/v1/agents/:name/run
casper agent list               → GET /api/v1/agents
casper agent create             → POST /api/v1/agents
casper agent update <n>      → PATCH /api/v1/agents/:name
casper agent delete <n>      → DELETE /api/v1/agents/:name
casper agent export <n>      → GET /api/v1/agents/:name/export
casper agent import <file>      → POST /api/v1/agents/import

casper deploy list              → GET /api/v1/deployments
casper deploy create            → POST /api/v1/deployments
casper deploy test <id>         → POST /api/v1/deployments/:id/test

casper knowledge upload <file>  → POST /api/v1/knowledge
casper knowledge list           → GET /api/v1/knowledge
casper knowledge search <query> → POST /api/v1/knowledge/search

casper memory show <agent>      → GET /api/v1/agents/:name/memory
casper memory update <agent>    → PUT /api/v1/agents/:name/memory
casper memory history <agent>   → GET /api/v1/agents/:name/memory/history
casper tenant-memory show       → GET /api/v1/tenant-memory
casper tenant-memory update     → PUT /api/v1/tenant-memory

casper secrets set <key>        → POST /api/v1/secrets
casper secrets list             → GET /api/v1/secrets
casper secrets delete <key>     → DELETE /api/v1/secrets/:key

casper keys create              → POST /api/v1/api-keys
casper keys list                → GET /api/v1/api-keys
casper keys update <id>         → PATCH /api/v1/api-keys/:id
casper keys revoke <id>         → DELETE /api/v1/api-keys/:id

casper feedback submit          → POST /api/v1/feedback
casper training export          → GET /api/v1/training/export
casper conversation outcome <id> → PATCH /api/v1/conversations/:id/outcome

casper audit query              → GET /api/v1/audit
casper usage summary            → GET /api/v1/usage
casper conversations list       → GET /api/v1/conversations
casper snippets list            → GET /api/v1/snippets
```

---

*End of API specification.*
