-- ============================================================
-- 0005_user_connections: OAuth providers + per-user connections
-- ============================================================

-- 1. oauth_providers — platform scope (no RLS)
-- Ventoo-managed OAuth application registrations.
CREATE TABLE oauth_providers (
    id                  UUID PRIMARY KEY,
    name                TEXT UNIQUE NOT NULL,       -- "microsoft365", "google", "slack", "github"
    display_name        TEXT NOT NULL,
    authorization_url   TEXT NOT NULL,
    token_url           TEXT NOT NULL,
    revocation_url      TEXT,
    client_id           TEXT NOT NULL,
    client_secret_enc   TEXT NOT NULL,              -- AES-256-GCM encrypted via vault
    default_scopes      TEXT NOT NULL,              -- space-separated OAuth scopes
    icon_url            TEXT,
    is_active           BOOLEAN NOT NULL DEFAULT true,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. user_connections — tenant scope (RLS enforced)
-- Per-user OAuth tokens for MCP server authentication.
CREATE TABLE user_connections (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenants(id),
    user_subject        TEXT NOT NULL,              -- "user:jane@acme.com"
    provider            TEXT NOT NULL REFERENCES oauth_providers(name),
    access_token_enc    TEXT NOT NULL,              -- AES-256-GCM with HKDF per-tenant key
    refresh_token_enc   TEXT,                       -- nullable (not all providers issue refresh tokens)
    token_expires_at    TIMESTAMPTZ,                -- access token expiry
    granted_scopes      TEXT NOT NULL,              -- actual scopes granted
    external_user_id    TEXT,                       -- provider's user ID
    external_email      TEXT,                       -- email on the provider side
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, user_subject, provider)
);

ALTER TABLE user_connections ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_connections_isolation ON user_connections
    USING (tenant_id = current_tenant_id());

CREATE INDEX idx_user_connections_user ON user_connections(tenant_id, user_subject);
CREATE INDEX idx_user_connections_provider ON user_connections(tenant_id, provider);
