-- ============================================================
-- 0007_mcp_connections: Centralized MCP server connections
-- ============================================================

-- Tenant-scoped MCP server connections. Agents reference these by name
-- instead of embedding connection details inline in their tools JSONB.
CREATE TABLE mcp_connections (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    url             TEXT NOT NULL,
    auth_type       TEXT NOT NULL DEFAULT 'none',
    api_key_enc     TEXT,
    auth_provider   TEXT,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name)
);

ALTER TABLE mcp_connections ENABLE ROW LEVEL SECURITY;
CREATE POLICY mcp_connections_isolation ON mcp_connections
    USING (tenant_id = current_tenant_id());

CREATE INDEX idx_mcp_connections_active ON mcp_connections(tenant_id, name)
    WHERE is_active = true;
