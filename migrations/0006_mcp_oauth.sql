-- ============================================================
-- 0006_mcp_oauth: MCP OAuth 2.1 auto-discovered registrations
-- ============================================================

-- Per-tenant DCR registrations + cached metadata for MCP OAuth 2.1.
-- Keyed by (tenant, mcp_server_url, as_issuer).
CREATE TABLE mcp_oauth_registrations (
    id                      UUID PRIMARY KEY,
    tenant_id               UUID NOT NULL REFERENCES tenants(id),
    mcp_server_url          TEXT NOT NULL,
    as_issuer               TEXT NOT NULL,

    -- Cached metadata
    prm_json                JSONB NOT NULL,
    as_metadata_json        JSONB NOT NULL,

    -- DCR result (client_id cached per tenant)
    client_id               TEXT NOT NULL,
    client_secret_enc       TEXT,

    metadata_fetched_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, mcp_server_url, as_issuer)
);

ALTER TABLE mcp_oauth_registrations ENABLE ROW LEVEL SECURITY;
CREATE POLICY mcp_oauth_registrations_isolation ON mcp_oauth_registrations
    USING (tenant_id = current_tenant_id());

-- Relax the FK on user_connections.provider so it can hold auto-discovered keys
ALTER TABLE user_connections DROP CONSTRAINT IF EXISTS user_connections_provider_fkey;

-- Add provider_type to distinguish manual vs mcp_oauth connections
ALTER TABLE user_connections ADD COLUMN IF NOT EXISTS provider_type TEXT NOT NULL DEFAULT 'manual';
