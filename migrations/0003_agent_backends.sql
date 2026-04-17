-- Agent Backend Keys (platform scope, no RLS)
-- Allows self-hosted GPU servers to connect via WebSocket for inference.

CREATE TABLE agent_backend_keys (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    key_hash        TEXT NOT NULL,
    key_prefix      TEXT NOT NULL,
    backend_id      UUID NOT NULL REFERENCES platform_backends(id),
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_agent_backend_keys_hash ON agent_backend_keys(key_hash);

-- Grant access to the casper application user
GRANT ALL ON agent_backend_keys TO casper;
