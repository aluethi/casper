-- Casper Platform Tables (no RLS)
-- 7 tables: tenants, sso_providers, email_domains, models, platform_backends,
--           platform_backend_models, token_revocations

-- pgvector extension (required for knowledge base, created in 0002 if available)
-- CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- 1. tenants
CREATE TABLE tenants (
    id              UUID PRIMARY KEY,
    slug            TEXT UNIQUE NOT NULL,
    display_name    TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',
    settings        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. sso_providers
CREATE TABLE sso_providers (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    provider_type   TEXT NOT NULL DEFAULT 'oidc',
    issuer_url      TEXT NOT NULL,
    client_id       TEXT NOT NULL,
    client_secret_enc TEXT NOT NULL,
    scopes          TEXT NOT NULL DEFAULT 'openid email profile',
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id)
);

-- 3. email_domains
CREATE TABLE email_domains (
    domain          TEXT PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    verified        BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_email_domains_tenant ON email_domains(tenant_id);

-- 4. models
CREATE TABLE models (
    id                      UUID PRIMARY KEY,
    name                    TEXT UNIQUE NOT NULL,
    display_name            TEXT NOT NULL,
    provider                TEXT NOT NULL,

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
    embedding_dimensions    INT,

    -- Pricing (dimensionless, implicitly CHF)
    cost_per_1k_input       NUMERIC(10,6),
    cost_per_1k_output      NUMERIC(10,6),
    cost_per_1k_cache_read  NUMERIC(10,6),
    cost_per_1k_cache_write NUMERIC(10,6),

    published               BOOLEAN NOT NULL DEFAULT false,
    is_active               BOOLEAN NOT NULL DEFAULT true,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 5. platform_backends
CREATE TABLE platform_backends (
    id              UUID PRIMARY KEY,
    name            TEXT UNIQUE NOT NULL,
    provider        TEXT NOT NULL,
    provider_label  TEXT,
    base_url        TEXT,
    api_key_enc     TEXT,
    region          TEXT,
    priority        INT NOT NULL DEFAULT 100,
    max_queue_depth INT NOT NULL DEFAULT 0,
    extra_config    JSONB NOT NULL DEFAULT '{}',
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 6. platform_backend_models
CREATE TABLE platform_backend_models (
    backend_id      UUID NOT NULL REFERENCES platform_backends(id) ON DELETE CASCADE,
    model_id        UUID NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    priority        INT NOT NULL DEFAULT 100,
    PRIMARY KEY (backend_id, model_id)
);

-- 7. token_revocations
CREATE TABLE token_revocations (
    jti             TEXT PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    revoked_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_by      TEXT NOT NULL
);

CREATE INDEX idx_token_revocations_tenant ON token_revocations(tenant_id);
