# Casper Platform — Data Model v1

**Version:** 1.0 | **Date:** April 2026 | **Status:** Draft
**Classification:** Internal — Confidential

---

## 1. Overview

The Casper data model serves two product surfaces from a single PostgreSQL 16 database with pgvector:

**Inference layer** — model catalog (structured capabilities, cache pricing, published flag), platform backends, model quotas, model deployments, API keys with three-part scopes, usage tracking with cache token accounting.

**Agent layer** — agent definitions with composable prompt stacks, all-object tool configuration, conversations, messages, knowledge base (files on disk, chunks + embeddings in DB), versioned markdown memory (agent + tenant), snippets, datasource blocks, feedback, audit log.

### 1.1 Design Principles

- **Tenant isolation via RLS.** Every tenant-scoped table has `tenant_id` and an RLS policy.
- **Ventoo-operated inference.** All backends platform-managed. Tenants access via quotas.
- **Agents are database-defined.** Portable via export/import.
- **MCP-first for external tools.** No sandbox/CLI infrastructure. Everything external goes through MCP servers.
- **Memory is a living document.** Versioned markdown, not key-value stores.
- **Three-part scopes.** `resource:identifier:action` for fine-grained access control on API keys and users.
- **Dimensionless pricing.** No currency field. Implicitly CHF. Forex handled at billing time (V2).
- **Every interaction is training data.** Feedback provides quality signals.

### 1.2 Conventions

- **UUIDs:** v7 (time-ordered) for all primary keys.
- **Timestamps:** `TIMESTAMPTZ`, stored as UTC.
- **Soft deletes:** `is_active BOOLEAN DEFAULT true`.
- **RLS function:** `current_tenant_id()` reads the session variable set by `TenantDb`.

### 1.3 Entity Relationships

```
tenants
  ├── sso_providers
  ├── email_domains
  ├── tenant_users
  ├── api_keys
  ├── tenant_secrets
  ├── model_quotas ──────────────── models
  ├── model_deployments ─────────── models
  │     (backend_sequence refs platform_backends via platform_backend_models)
  ├── agents
  │     (prompt_stack refs snippets, datasources)
  ├── conversations
  │     └── messages
  │           └── message_feedback
  ├── agent_memory ──── agent_memory_history
  ├── tenant_memory ── tenant_memory_history
  ├── snippets
  ├── agent_state
  ├── documents
  │     └── document_chunks
  ├── audit_log
  └── usage_events

models
  └── platform_backend_models ──── platform_backends
```

---

## 2. Platform Tables (No RLS)

### 2.1 tenants

```sql
CREATE TABLE tenants (
    id              UUID PRIMARY KEY,
    slug            TEXT UNIQUE NOT NULL,
    display_name    TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',   -- active, suspended, deactivated
    settings        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 2.2 sso_providers

Per-tenant OIDC configuration. One provider per tenant (V1).

```sql
CREATE TABLE sso_providers (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    provider_type   TEXT NOT NULL DEFAULT 'oidc',
    issuer_url      TEXT NOT NULL,
    client_id       TEXT NOT NULL,
    client_secret_enc TEXT NOT NULL,            -- AES-256-GCM encrypted
    scopes          TEXT NOT NULL DEFAULT 'openid email profile',
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id)
);
```

### 2.3 email_domains

Maps email domains to tenants for SSO discovery.

```sql
CREATE TABLE email_domains (
    domain          TEXT PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    verified        BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_email_domains_tenant ON email_domains(tenant_id);
```

### 2.4 models

Platform model catalog. Structured capability columns. Dimensionless pricing with cache support.

```sql
CREATE TABLE models (
    id                      UUID PRIMARY KEY,
    name                    TEXT UNIQUE NOT NULL,
    display_name            TEXT NOT NULL,
    provider                TEXT NOT NULL,          -- anthropic, openai, azure_openai, mistral, self_hosted

    -- Capabilities (structured, queryable)
    cap_chat                BOOLEAN NOT NULL DEFAULT false,
    cap_embedding           BOOLEAN NOT NULL DEFAULT false,
    cap_thinking            BOOLEAN NOT NULL DEFAULT false,
    cap_vision              BOOLEAN NOT NULL DEFAULT false,
    cap_tool_use            BOOLEAN NOT NULL DEFAULT false,
    cap_json_output         BOOLEAN NOT NULL DEFAULT false,
    cap_audio_in            BOOLEAN NOT NULL DEFAULT false,
    cap_audio_out           BOOLEAN NOT NULL DEFAULT false,
    cap_image_gen           BOOLEAN NOT NULL DEFAULT false,

    -- Numeric capabilities
    context_window          INT,
    max_output_tokens       INT,
    embedding_dimensions    INT,                   -- for embedding models

    -- Pricing (dimensionless, implicitly CHF)
    cost_per_1k_input       NUMERIC(10,6),
    cost_per_1k_output      NUMERIC(10,6),
    cost_per_1k_cache_read  NUMERIC(10,6),
    cost_per_1k_cache_write NUMERIC(10,6),

    published               BOOLEAN NOT NULL DEFAULT false,
    is_active               BOOLEAN NOT NULL DEFAULT true,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 2.5 platform_backends

Ventoo-operated inference backends.

```sql
CREATE TABLE platform_backends (
    id              UUID PRIMARY KEY,
    name            TEXT UNIQUE NOT NULL,
    provider        TEXT NOT NULL,              -- anthropic, openai_compatible
    provider_label  TEXT,                       -- display: "OpenAI", "Azure OpenAI", "Mistral", "Ollama"
    base_url        TEXT,
    api_key_enc     TEXT,                       -- AES-256-GCM encrypted
    region          TEXT,
    priority        INT NOT NULL DEFAULT 100,
    max_queue_depth INT NOT NULL DEFAULT 0,
    extra_config    JSONB NOT NULL DEFAULT '{}',
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 2.6 platform_backend_models

N:M: which backends serve which models.

```sql
CREATE TABLE platform_backend_models (
    backend_id      UUID NOT NULL REFERENCES platform_backends(id) ON DELETE CASCADE,
    model_id        UUID NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    priority        INT NOT NULL DEFAULT 100,
    PRIMARY KEY (backend_id, model_id)
);
```

### 2.7 token_revocations

JWT revocation list. Cached in-memory, refreshed periodically.

```sql
CREATE TABLE token_revocations (
    jti             TEXT PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    revoked_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_by      TEXT NOT NULL
);

CREATE INDEX idx_token_revocations_tenant ON token_revocations(tenant_id);
```

---

## 3. Tenant-Scoped Tables (RLS Enforced)

```sql
CREATE OR REPLACE FUNCTION current_tenant_id() RETURNS UUID AS $$
    SELECT current_setting('app.tenant_id', true)::UUID;
$$ LANGUAGE SQL STABLE;
```

### 3.1 Auth & Identity

#### tenant_users

```sql
CREATE TABLE tenant_users (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    subject         TEXT NOT NULL,
    role            TEXT NOT NULL,              -- owner, admin, operator, viewer
    scopes          TEXT[] NOT NULL,            -- three-part: "agents:triage:run"
    email           TEXT,
    display_name    TEXT,
    last_login_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL,
    UNIQUE (tenant_id, subject)
);
```

#### api_keys

Single machine-to-machine auth mechanism. Three-part scopes for resource restriction.

```sql
CREATE TABLE api_keys (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    scopes          TEXT[] NOT NULL,            -- ["inference:sonnet-fast:call", "agents:triage:run"]
    key_hash        TEXT NOT NULL,
    key_prefix      TEXT NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL,
    UNIQUE (tenant_id, name)
);

CREATE UNIQUE INDEX idx_api_keys_hash ON api_keys(key_hash);
```

### 3.2 Secrets

#### tenant_secrets

Outbound credentials for MCP server authentication.

```sql
CREATE TABLE tenant_secrets (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    key             TEXT NOT NULL,
    ciphertext_b64  TEXT NOT NULL,
    nonce_b64       TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, key)
);
```

### 3.3 Inference Configuration

#### model_quotas

Per-tenant, per-model allocation with cache limits and pricing overrides.

```sql
CREATE TABLE model_quotas (
    tenant_id               UUID NOT NULL REFERENCES tenants(id),
    model_id                UUID NOT NULL REFERENCES models(id),
    requests_per_minute     INT NOT NULL DEFAULT 0,
    tokens_per_day          BIGINT NOT NULL DEFAULT 0,
    cache_tokens_per_day    BIGINT NOT NULL DEFAULT 0,
    cost_per_1k_input       NUMERIC(10,6),     -- NULL = model default
    cost_per_1k_output      NUMERIC(10,6),
    cost_per_1k_cache_read  NUMERIC(10,6),
    cost_per_1k_cache_write NUMERIC(10,6),
    allocated_by            TEXT NOT NULL,
    allocated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, model_id)
);
```

#### model_deployments

Tenant-scoped routing configuration.

```sql
CREATE TABLE model_deployments (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenants(id),
    model_id            UUID NOT NULL REFERENCES models(id),
    name                TEXT NOT NULL,
    slug                TEXT NOT NULL,
    backend_sequence    UUID[] NOT NULL DEFAULT '{}',
    retry_attempts      INT NOT NULL DEFAULT 1,
    retry_backoff_ms    INT NOT NULL DEFAULT 1000,
    fallback_enabled    BOOLEAN NOT NULL DEFAULT true,
    timeout_ms          INT NOT NULL DEFAULT 30000,
    default_params      JSONB NOT NULL DEFAULT '{}',
    rate_limit_rpm      INT,
    is_active           BOOLEAN NOT NULL DEFAULT true,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, slug)
);

CREATE INDEX idx_deployments_slug ON model_deployments(tenant_id, slug)
    WHERE is_active = true;
```

### 3.4 Agents

#### agents

```sql
CREATE TABLE agents (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    description     TEXT,
    model_deployment TEXT NOT NULL,             -- slug → model_deployments
    prompt_stack    JSONB NOT NULL DEFAULT '[]',
    tools           JSONB NOT NULL DEFAULT '{}',
    config          JSONB NOT NULL DEFAULT '{}',
    version         INT NOT NULL DEFAULT 1,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL,
    UNIQUE (tenant_id, name)
);
```

**`prompt_stack`** — ordered array of blocks:

| Type | Purpose |
|---|---|
| `text` | Static instructions (markdown) |
| `environment` | Runtime context (datetime, tenant, source) |
| `agent_memory` | Agent's versioned memory document |
| `tenant_memory` | Shared tenant memory document (read-only for agents) |
| `knowledge` | RAG retrieval from knowledge base |
| `delegates` | Available sub-agents with descriptions |
| `datasource` | External data fetched at assembly time via MCP or HTTP, with `{{metadata.*}}` template variables |
| `snippet` | Reusable block from snippets library |
| `variable` | Custom key-value pair |

**`tools`** — all built-in tools are objects with configuration:

```json
{
  "builtin": [
    { "name": "delegate", "timeout_secs": 300, "max_depth": 3 },
    { "name": "ask_user", "notification": {
        "mcp_server": "Slack", "tool": "send_message",
        "params": { "channel": "#incidents", "text": "Agent {{agent.name}} needs input: {{question}}\n{{conversation.url}}" }
    }},
    { "name": "knowledge_search", "max_results": 5, "relevance_threshold": 0.7 },
    { "name": "update_memory", "max_document_tokens": 4000 },
    { "name": "web_search", "max_results": 10 },
    { "name": "web_fetch", "timeout_secs": 30, "max_response_bytes": 1048576 }
  ],
  "mcp": [{
    "name": "Ventoo Relay",
    "url": "https://mcp.ventoo.ai/relay/mcp",
    "auth": { "type": "bearer", "token_ref": "secret:relay_service_token" },
    "enabled_tools": ["connect_device", "relay_exec", "relay_screenshot", "disconnect_device"]
  }]
}
```

**`config`** — global agent settings (tool-specific settings live on the tools):

```json
{ "max_turns": 20 }
```

#### agent_state

Actor runtime state per conversation.

```sql
CREATE TABLE agent_state (
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    conversation_id UUID NOT NULL,
    state           JSONB NOT NULL DEFAULT '{}',
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, agent_name, conversation_id)
);
```

#### agent_memory

Single versioned markdown document per agent.

```sql
CREATE TABLE agent_memory (
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    token_count     INT NOT NULL DEFAULT 0,
    version         INT NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, agent_name)
);
```

#### agent_memory_history

```sql
CREATE TABLE agent_memory_history (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    version         INT NOT NULL,
    content         TEXT NOT NULL,
    token_count     INT NOT NULL,
    updated_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, agent_name, version)
);
```

#### tenant_memory

Single versioned markdown document per tenant. Updated by humans only.

```sql
CREATE TABLE tenant_memory (
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    content         TEXT NOT NULL DEFAULT '',
    token_count     INT NOT NULL DEFAULT 0,
    version         INT NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (tenant_id)
);
```

#### tenant_memory_history

```sql
CREATE TABLE tenant_memory_history (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    version         INT NOT NULL,
    content         TEXT NOT NULL,
    token_count     INT NOT NULL,
    updated_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, version)
);
```

#### snippets

```sql
CREATE TABLE snippets (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    content         TEXT NOT NULL,
    token_estimate  INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL,
    UNIQUE (tenant_id, name)
);
```

### 3.5 Conversations

#### conversations

Each conversation is an actor instance. Multiple users can participate.

```sql
CREATE TABLE conversations (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',    -- active, waiting, completed
    title           TEXT,
    outcome         TEXT,                              -- resolved, unresolved, escalated, duplicate
    outcome_notes   TEXT,
    outcome_by      TEXT,
    outcome_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversations_agent ON conversations(tenant_id, agent_name, updated_at DESC);
CREATE INDEX idx_conversations_status ON conversations(tenant_id, status) WHERE status = 'waiting';
CREATE INDEX idx_conversations_outcome ON conversations(tenant_id, outcome) WHERE outcome IS NOT NULL;
```

#### messages

```sql
CREATE TABLE messages (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL,              -- user, assistant, tool_use, tool_result
    author          TEXT,                       -- "user:alice@ventoo.ch", NULL for assistant/tool
    content         JSONB NOT NULL,
    token_count     INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, created_at);
```

### 3.6 Feedback

#### message_feedback

```sql
CREATE TABLE message_feedback (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    message_id      UUID NOT NULL REFERENCES messages(id),
    conversation_id UUID NOT NULL REFERENCES conversations(id),
    agent_name      TEXT NOT NULL,
    feedback_type   TEXT NOT NULL,              -- rating, correction, tag
    rating          INT,                        -- +1 or -1
    correction      TEXT,
    tags            TEXT[],
    created_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_feedback_agent ON message_feedback(tenant_id, agent_name);
CREATE INDEX idx_feedback_rating ON message_feedback(tenant_id, rating);
```

**Quality view for training data export:**

```sql
CREATE VIEW conversation_quality AS
SELECT
    c.id, c.tenant_id, c.agent_name, c.outcome, c.created_at,
    COUNT(m.id) FILTER (WHERE m.role = 'user') as user_turns,
    COUNT(m.id) FILTER (WHERE m.role = 'tool_result') as tool_calls,
    COUNT(m.id) FILTER (WHERE m.role = 'tool_result'
        AND (m.content->>'is_error')::boolean = false) as tool_successes,
    COALESCE(AVG(f.rating), 0) as avg_rating,
    COUNT(f.id) FILTER (WHERE f.feedback_type = 'correction') as correction_count,
    bool_or(COALESCE(f.tags @> ARRAY['hallucination'], false)) as has_hallucination,
    CASE
        WHEN COUNT(f.id) FILTER (WHERE f.feedback_type = 'correction') > 0 THEN 'gold'
        WHEN COALESCE(AVG(f.rating), 0) >= 0.8 AND c.outcome = 'resolved'
             AND NOT bool_or(COALESCE(f.tags @> ARRAY['hallucination'], false)) THEN 'silver'
        WHEN c.outcome = 'resolved' AND COALESCE(AVG(f.rating), 0) >= 0 THEN 'bronze'
        ELSE 'excluded'
    END as quality_tier
FROM conversations c
LEFT JOIN messages m ON m.conversation_id = c.id
LEFT JOIN message_feedback f ON f.conversation_id = c.id
GROUP BY c.id;
```

### 3.7 Knowledge Base

Source files stored on filesystem: `data/knowledge/{tenant_id}/{document_id}.{ext}`.

#### documents

```sql
CREATE TABLE documents (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    source          TEXT,
    content_type    TEXT NOT NULL,
    file_path       TEXT NOT NULL,             -- "data/knowledge/{tenant_id}/{id}.pdf"
    token_count     INT,
    chunk_count     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL
);

CREATE INDEX idx_documents_tenant ON documents(tenant_id);
```

#### document_chunks

```sql
CREATE TABLE document_chunks (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    document_id     UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index     INT NOT NULL,
    content         TEXT NOT NULL,
    embedding       vector(1536),
    token_count     INT NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_chunks_document ON document_chunks(document_id, chunk_index);
CREATE INDEX idx_chunks_embedding ON document_chunks
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);
```

### 3.8 Observability

#### audit_log

```sql
CREATE TABLE audit_log (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    timestamp       TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor           TEXT NOT NULL,
    action          TEXT NOT NULL,
    resource        TEXT,
    detail          JSONB NOT NULL DEFAULT '{}',
    outcome         TEXT NOT NULL,             -- success, failure, denied
    correlation_id  UUID NOT NULL,
    token_id        TEXT NOT NULL
);

CREATE INDEX idx_audit_tenant_time ON audit_log(tenant_id, timestamp DESC);
CREATE INDEX idx_audit_correlation ON audit_log(tenant_id, correlation_id);
CREATE INDEX idx_audit_action ON audit_log(tenant_id, action, timestamp DESC);
```

#### usage_events

Tracks cache token usage alongside standard input/output.

```sql
CREATE TABLE usage_events (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    source          TEXT NOT NULL,             -- api, agent, playground
    model           TEXT NOT NULL,
    deployment_slug TEXT,
    agent_name      TEXT,
    input_tokens    INT NOT NULL,
    output_tokens   INT NOT NULL,
    cache_read_tokens  INT,
    cache_write_tokens INT,
    backend_id      UUID,
    correlation_id  UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_usage_tenant_day ON usage_events(tenant_id, (created_at::date));
CREATE INDEX idx_usage_tenant_model ON usage_events(tenant_id, model);
CREATE INDEX idx_usage_tenant_agent ON usage_events(tenant_id, agent_name)
    WHERE agent_name IS NOT NULL;
```

---

## 4. Server Configuration

Platform-level settings loaded from server config file, not per-tenant.

```yaml
# casper-server.yaml

web_search:
  base_url: https://search.ventoo.ai       # SearXNG instance
  format: json
  max_results: 10

web_fetch:
  timeout_secs: 30
  max_response_bytes: 1048576

knowledge:
  storage_path: ./data/knowledge            # base path for document files
  embedding_deployment: embeddings-default   # deployment slug for embedding generation
```

---

## 5. Prompt Stack (Logical Model)

The prompt assembler walks `prompt_stack` top to bottom. Order determines LLM attention priority.

**Budget allocation:**
1. Static blocks (text, environment, variable, snippet, delegates): always included
2. Datasource blocks: fetched at assembly time, budget-capped
3. Knowledge: fill up to budget with highest-ranked chunks
4. Agent memory: full document loaded (agent manages its own length via `max_document_tokens`)
5. Tenant memory: full document loaded
6. Conversation history: gets remaining budget — most recent turns first
7. Current message: always included

**Template variables:** `{{tenant.display_name}}`, `{{tenant.slug}}`, `{{agent.name}}`, `{{agent.display_name}}`, `{{metadata.*}}`.

---

## 6. Actor Model (Logical Model)

In-memory `DashMap` registry.

```
Actor key: (tenant_id, agent_name, conversation_id)
```

Each conversation is its own isolated actor. Agent memory shared at `(tenant_id, agent_name)` level.

**Lifecycle:** First request spawns tokio task → mailbox loop → sequential processing → idle reaper dehydrates → next request reactivates.

**Delegation:** Parallel via `JoinSet` for concurrent `delegate` tool calls. Depth and timeout configured per-tool on the `delegate` tool object.

---

## 7. Summary

| Category | Tables | Count |
|---|---|---|
| Platform (no RLS) | `tenants`, `sso_providers`, `email_domains`, `models`, `platform_backends`, `platform_backend_models`, `token_revocations` | 7 |
| Auth & Identity | `tenant_users`, `api_keys` | 2 |
| Secrets | `tenant_secrets` | 1 |
| Inference Config | `model_quotas`, `model_deployments` | 2 |
| Agents | `agents`, `agent_state`, `agent_memory`, `agent_memory_history`, `tenant_memory`, `tenant_memory_history`, `snippets` | 7 |
| Conversations | `conversations`, `messages` | 2 |
| Feedback | `message_feedback` | 1 |
| Knowledge | `documents`, `document_chunks` | 2 |
| Observability | `audit_log`, `usage_events` | 2 |
| **Total** | | **26** |
| Views | `conversation_quality` | 1 |

### Future Considerations

- **ReBAC:** Relationship-based access control via `relationships` table for per-agent/per-knowledge access control.
- **Tenant-owned backends:** Tenants bring their own API keys for LLM providers.
- **Inventory API:** Serial-to-asset-tag lookup for Ventoo Agent onboarding.
- **Billing:** Forex conversion, invoicing, payment integration.

---

*End of data model specification.*
