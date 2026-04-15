-- Casper Tenant-Scoped Tables (RLS Enforced)
-- 19 tables + 1 view: tenant_users, api_keys, tenant_secrets, model_quotas,
-- model_deployments, agents, agent_state, agent_memory, agent_memory_history,
-- tenant_memory, tenant_memory_history, snippets, conversations, messages,
-- message_feedback, documents, document_chunks, audit_log, usage_events
-- + conversation_quality view

-- RLS helper function
CREATE OR REPLACE FUNCTION current_tenant_id() RETURNS UUID AS $$
    SELECT current_setting('app.tenant_id', true)::UUID;
$$ LANGUAGE SQL STABLE;

-- ============================================================
-- 3.1 Auth & Identity
-- ============================================================

-- 1. tenant_users
CREATE TABLE tenant_users (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    subject         TEXT NOT NULL,
    role            TEXT NOT NULL,
    scopes          TEXT[] NOT NULL,
    email           TEXT,
    display_name    TEXT,
    last_login_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL,
    UNIQUE (tenant_id, subject)
);

ALTER TABLE tenant_users ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_users_isolation ON tenant_users
    USING (tenant_id = current_tenant_id());

-- 2. api_keys
CREATE TABLE api_keys (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    scopes          TEXT[] NOT NULL,
    key_hash        TEXT NOT NULL,
    key_prefix      TEXT NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL,
    UNIQUE (tenant_id, name)
);

CREATE UNIQUE INDEX idx_api_keys_hash ON api_keys(key_hash);

ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
CREATE POLICY api_keys_isolation ON api_keys
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.2 Secrets
-- ============================================================

-- 3. tenant_secrets
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

ALTER TABLE tenant_secrets ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_secrets_isolation ON tenant_secrets
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.3 Inference Configuration
-- ============================================================

-- 4. model_quotas
CREATE TABLE model_quotas (
    tenant_id               UUID NOT NULL REFERENCES tenants(id),
    model_id                UUID NOT NULL REFERENCES models(id),
    requests_per_minute     INT NOT NULL DEFAULT 0,
    tokens_per_day          BIGINT NOT NULL DEFAULT 0,
    cache_tokens_per_day    BIGINT NOT NULL DEFAULT 0,
    cost_per_1k_input       NUMERIC(10,6),
    cost_per_1k_output      NUMERIC(10,6),
    cost_per_1k_cache_read  NUMERIC(10,6),
    cost_per_1k_cache_write NUMERIC(10,6),
    allocated_by            TEXT NOT NULL,
    allocated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, model_id)
);

ALTER TABLE model_quotas ENABLE ROW LEVEL SECURITY;
CREATE POLICY model_quotas_isolation ON model_quotas
    USING (tenant_id = current_tenant_id());

-- 5. model_deployments
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

ALTER TABLE model_deployments ENABLE ROW LEVEL SECURITY;
CREATE POLICY model_deployments_isolation ON model_deployments
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.4 Agents
-- ============================================================

-- 6. agents
CREATE TABLE agents (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    description     TEXT,
    model_deployment TEXT NOT NULL,
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

ALTER TABLE agents ENABLE ROW LEVEL SECURITY;
CREATE POLICY agents_isolation ON agents
    USING (tenant_id = current_tenant_id());

-- 7. agent_state
CREATE TABLE agent_state (
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    conversation_id UUID NOT NULL,
    state           JSONB NOT NULL DEFAULT '{}',
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, agent_name, conversation_id)
);

ALTER TABLE agent_state ENABLE ROW LEVEL SECURITY;
CREATE POLICY agent_state_isolation ON agent_state
    USING (tenant_id = current_tenant_id());

-- 8. agent_memory
CREATE TABLE agent_memory (
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    token_count     INT NOT NULL DEFAULT 0,
    version         INT NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, agent_name)
);

ALTER TABLE agent_memory ENABLE ROW LEVEL SECURITY;
CREATE POLICY agent_memory_isolation ON agent_memory
    USING (tenant_id = current_tenant_id());

-- 9. agent_memory_history
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

ALTER TABLE agent_memory_history ENABLE ROW LEVEL SECURITY;
CREATE POLICY agent_memory_history_isolation ON agent_memory_history
    USING (tenant_id = current_tenant_id());

-- 10. tenant_memory
CREATE TABLE tenant_memory (
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    content         TEXT NOT NULL DEFAULT '',
    token_count     INT NOT NULL DEFAULT 0,
    version         INT NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (tenant_id)
);

ALTER TABLE tenant_memory ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_memory_isolation ON tenant_memory
    USING (tenant_id = current_tenant_id());

-- 11. tenant_memory_history
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

ALTER TABLE tenant_memory_history ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_memory_history_isolation ON tenant_memory_history
    USING (tenant_id = current_tenant_id());

-- 12. snippets
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

ALTER TABLE snippets ENABLE ROW LEVEL SECURITY;
CREATE POLICY snippets_isolation ON snippets
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.5 Conversations
-- ============================================================

-- 13. conversations
CREATE TABLE conversations (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    agent_name      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',
    title           TEXT,
    outcome         TEXT,
    outcome_notes   TEXT,
    outcome_by      TEXT,
    outcome_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversations_agent ON conversations(tenant_id, agent_name, updated_at DESC);
CREATE INDEX idx_conversations_status ON conversations(tenant_id, status) WHERE status = 'waiting';
CREATE INDEX idx_conversations_outcome ON conversations(tenant_id, outcome) WHERE outcome IS NOT NULL;

ALTER TABLE conversations ENABLE ROW LEVEL SECURITY;
CREATE POLICY conversations_isolation ON conversations
    USING (tenant_id = current_tenant_id());

-- 14. messages
CREATE TABLE messages (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL,
    author          TEXT,
    content         JSONB NOT NULL,
    token_count     INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, created_at);

ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
CREATE POLICY messages_isolation ON messages
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.6 Feedback
-- ============================================================

-- 15. message_feedback
CREATE TABLE message_feedback (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    message_id      UUID NOT NULL REFERENCES messages(id),
    conversation_id UUID NOT NULL REFERENCES conversations(id),
    agent_name      TEXT NOT NULL,
    feedback_type   TEXT NOT NULL,
    rating          INT,
    correction      TEXT,
    tags            TEXT[],
    created_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_feedback_agent ON message_feedback(tenant_id, agent_name);
CREATE INDEX idx_feedback_rating ON message_feedback(tenant_id, rating);

ALTER TABLE message_feedback ENABLE ROW LEVEL SECURITY;
CREATE POLICY message_feedback_isolation ON message_feedback
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.7 Knowledge Base
-- ============================================================

-- 16. documents
CREATE TABLE documents (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    source          TEXT,
    content_type    TEXT NOT NULL,
    file_path       TEXT NOT NULL,
    token_count     INT,
    chunk_count     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL
);

CREATE INDEX idx_documents_tenant ON documents(tenant_id);

ALTER TABLE documents ENABLE ROW LEVEL SECURITY;
CREATE POLICY documents_isolation ON documents
    USING (tenant_id = current_tenant_id());

-- 17. document_chunks
-- Note: embedding column uses vector(1536) if pgvector is available
CREATE TABLE document_chunks (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    document_id     UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index     INT NOT NULL,
    content         TEXT NOT NULL,
    token_count     INT NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_chunks_document ON document_chunks(document_id, chunk_index);

ALTER TABLE document_chunks ENABLE ROW LEVEL SECURITY;
CREATE POLICY document_chunks_isolation ON document_chunks
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- 3.8 Observability
-- ============================================================

-- 18. audit_log
CREATE TABLE audit_log (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    timestamp       TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor           TEXT NOT NULL,
    action          TEXT NOT NULL,
    resource        TEXT,
    detail          JSONB NOT NULL DEFAULT '{}',
    outcome         TEXT NOT NULL,
    correlation_id  UUID NOT NULL,
    token_id        TEXT NOT NULL
);

CREATE INDEX idx_audit_tenant_time ON audit_log(tenant_id, timestamp DESC);
CREATE INDEX idx_audit_correlation ON audit_log(tenant_id, correlation_id);
CREATE INDEX idx_audit_action ON audit_log(tenant_id, action, timestamp DESC);

ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
CREATE POLICY audit_log_isolation ON audit_log
    USING (tenant_id = current_tenant_id());

-- 19. usage_events
CREATE TABLE usage_events (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    source          TEXT NOT NULL,
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

CREATE INDEX idx_usage_tenant_day ON usage_events(tenant_id, created_at);
CREATE INDEX idx_usage_tenant_model ON usage_events(tenant_id, model);
CREATE INDEX idx_usage_tenant_agent ON usage_events(tenant_id, agent_name)
    WHERE agent_name IS NOT NULL;

ALTER TABLE usage_events ENABLE ROW LEVEL SECURITY;
CREATE POLICY usage_events_isolation ON usage_events
    USING (tenant_id = current_tenant_id());

-- ============================================================
-- conversation_quality view
-- ============================================================

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
