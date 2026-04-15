# Casper Platform — Agent Backend Hosting

**Version:** 1.0 | **Date:** April 2026 | **Status:** Enhancement (not in V1 core)
**Classification:** Internal — Confidential
**Depends on:** Inference layer (Phase 3 of MILESTONES-v1.md)

---

## 1. Overview

Agent backends allow Ventoo to connect self-hosted GPU servers to the Casper inference layer. Instead of Casper calling an external HTTP API (Anthropic, OpenAI), the GPU server holds a persistent WebSocket connection to Casper and receives inference requests pushed to it.

From the routing engine's perspective, an agent backend is just another `platform_backend`. If it appears in a deployment's `backend_sequence`, requests route to it. The difference is the transport: HTTP call (standard backends) vs WebSocket push (agent backends).

### 1.1 Use Cases

- **Self-hosted open models:** Ventoo runs Llama, Mistral, Qwen on its own GPU servers and offers them to tenants alongside commercial APIs.
- **On-premise inference:** A GPU server in a customer's data center connects outbound to Casper — no inbound ports, no VPN needed.
- **Hybrid routing:** A deployment falls back from a commercial API to a self-hosted model (or vice versa) using the standard retry/fallback mechanism.
- **Cost optimization:** Route low-priority or batch requests to cheaper self-hosted backends.

### 1.2 Design Principles

- The agent backend is **outbound-only** — the GPU server connects to Casper, not the other way around. Works behind firewalls and NAT.
- The routing engine is **transport-agnostic** — it doesn't know whether a backend is HTTP or WebSocket. It calls `dispatch()` and gets a response.
- Agent backends use the **same model catalog** — a self-hosted Llama 3 is a `models` entry like any other. The backend is assigned to it via `platform_backend_models`.
- **Platform-managed only** — only Ventoo staff can register agent backends. Tenants cannot connect their own GPU servers (V1).

---

## 2. Architecture

```
Ventoo GPU Server                              casper-server
┌────────────────────────┐                    ┌──────────────────────┐
│  vLLM / Ollama / TGI   │                    │                      │
│                        │                    │  Routing Engine       │
│  casper-agent-backend  │───WSS────────────▶│    │                  │
│  (sidecar binary)      │                    │    ├── HTTP dispatch  │
│                        │◀─inference req────│    │   (Anthropic,    │
│                        │──response────────▶│    │    OpenAI)       │
│                        │                    │    │                  │
│  Auth: csa-<key>       │                    │    └── WS dispatch    │
│  Heartbeat: 30s        │                    │        (agent backends)│
└────────────────────────┘                    └──────────────────────┘
```

### 2.1 Components

**`casper-agent-backend`** — a lightweight sidecar binary that runs alongside the inference server (vLLM, Ollama, TGI). It:
1. Connects to Casper via WebSocket.
2. Receives inference requests.
3. Forwards them to the local inference server via HTTP (localhost).
4. Returns responses to Casper.

The sidecar translates between Casper's internal request format and whatever API the local inference server exposes.

**casper-server** — the existing server gains:
1. A WebSocket endpoint at `GET /agent/connect`.
2. An `AgentBackendRegistry` that tracks connected backends.
3. A WebSocket dispatch path in the routing engine (alongside HTTP dispatch).

---

## 3. Authentication

Agent backends authenticate with a dedicated key type: `csa-` prefix (Casper Service Agent).

```sql
-- New table (platform scope, no RLS)
CREATE TABLE agent_backend_keys (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    key_hash        TEXT NOT NULL,              -- SHA-256
    key_prefix      TEXT NOT NULL,              -- first 8 chars
    backend_id      UUID NOT NULL REFERENCES platform_backends(id),
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by      TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_agent_backend_keys_hash ON agent_backend_keys(key_hash);
```

Each key is tied to a specific `platform_backend`. When the sidecar connects, the key determines which backend it represents. Multiple sidecars can connect with the same key (horizontal scaling).

### 3.1 Key Management

```
POST   /api/v1/backends/:id/keys          # Create agent key (returns csa- key once)
GET    /api/v1/backends/:id/keys          # List keys (prefix only)
DELETE /api/v1/backends/:id/keys/:key_id  # Revoke
```

Scope: `platform:admin`.

---

## 4. WebSocket Protocol

### 4.1 Connection

```
GET /agent/connect
Authorization: Bearer csa-<key>
```

On successful auth:
1. Look up `agent_backend_keys` → find `backend_id`.
2. Verify backend exists and `is_active`.
3. Register the connection in `AgentBackendRegistry`.
4. Upgrade to WebSocket.

### 4.2 Registration Message

After WebSocket upgrade, the sidecar sends a registration message:

```json
{
  "type": "register",
  "backend_id": "...",
  "hostname": "gpu-server-01",
  "inference_server": "vllm",
  "inference_version": "0.4.0",
  "models_loaded": ["meta-llama/Llama-3-70B-Instruct"],
  "max_concurrent": 8,
  "gpu_info": {
    "count": 4,
    "type": "A100",
    "memory_gb": 80
  }
}
```

The server validates `backend_id` matches the authenticated key and stores the connection metadata. The `models_loaded` field is informational — routing is controlled by `platform_backend_models`, not by what the sidecar reports.

### 4.3 Heartbeat

```json
// Server → Sidecar (every 30s)
{ "type": "ping", "timestamp": "2026-04-11T10:20:00Z" }

// Sidecar → Server
{ "type": "pong", "timestamp": "2026-04-11T10:20:00Z", "active_requests": 3, "queue_depth": 0 }
```

The pong includes load metrics. The routing engine can use `active_requests` and `queue_depth` for load-aware routing (future enhancement — V1 uses round-robin across connected instances).

If no pong received within 90s, the connection is considered dead and removed from the registry.

### 4.4 Inference Request

```json
// Server → Sidecar
{
  "type": "inference_request",
  "id": "req-550e8400",
  "model": "meta-llama/Llama-3-70B-Instruct",
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "Hello!" }
  ],
  "params": {
    "max_tokens": 1024,
    "temperature": 0.7,
    "stream": false
  },
  "timeout_ms": 30000
}
```

The request format is provider-agnostic — the sidecar translates to whatever the local server expects (OpenAI-compatible for vLLM/Ollama, custom for TGI).

### 4.5 Inference Response (Non-Streaming)

```json
// Sidecar → Server
{
  "type": "inference_response",
  "id": "req-550e8400",
  "status": "ok",
  "message": {
    "role": "assistant",
    "content": "Hello! How can I help you today?"
  },
  "usage": {
    "input_tokens": 24,
    "output_tokens": 12,
    "cache_read_tokens": 0,
    "cache_write_tokens": 0
  },
  "duration_ms": 340
}
```

### 4.6 Inference Response (Streaming)

When `stream: true`, the sidecar sends chunks:

```json
// Sidecar → Server (multiple)
{ "type": "inference_chunk", "id": "req-550e8400", "delta": "Hello! " }
{ "type": "inference_chunk", "id": "req-550e8400", "delta": "How can " }
{ "type": "inference_chunk", "id": "req-550e8400", "delta": "I help you today?" }

// Final message with usage
{
  "type": "inference_done",
  "id": "req-550e8400",
  "usage": { "input_tokens": 24, "output_tokens": 12 },
  "duration_ms": 890
}
```

The server reassembles chunks and forwards them to the caller (SSE for HTTP inference API, or directly to the agent runtime).

### 4.7 Error Response

```json
{
  "type": "inference_error",
  "id": "req-550e8400",
  "error": "model_overloaded",
  "message": "All slots are busy. Queue depth: 12.",
  "retryable": true
}
```

`retryable: true` tells the routing engine to try the next backend in the sequence (standard fallback behavior).

### 4.8 Tool Use

When the model supports tool calling, the response includes tool_use blocks:

```json
{
  "type": "inference_response",
  "id": "req-550e8400",
  "status": "ok",
  "message": {
    "role": "assistant",
    "content": null,
    "tool_calls": [
      {
        "id": "call_abc123",
        "type": "function",
        "function": {
          "name": "get_weather",
          "arguments": "{\"location\":\"Zurich\"}"
        }
      }
    ]
  },
  "usage": { ... },
  "stop_reason": "tool_use"
}
```

---

## 5. Agent Backend Registry

In-memory registry tracking connected agent backends.

```rust
pub struct AgentBackendRegistry {
    /// backend_id → Vec of connected instances
    connections: DashMap<Uuid, Vec<AgentBackendConnection>>,
}

pub struct AgentBackendConnection {
    pub sender: mpsc::Sender<WebSocketMessage>,
    pub hostname: String,
    pub connected_at: Instant,
    pub active_requests: AtomicU32,
    pub max_concurrent: u32,
}
```

### 5.1 Operations

- `register(backend_id, connection)` — called on WebSocket connect.
- `unregister(backend_id, connection_id)` — called on disconnect.
- `is_available(backend_id) -> bool` — at least one connection with capacity.
- `dispatch(backend_id, request) -> Result<Response>` — pick a connection (round-robin), send request, await response via oneshot.

### 5.2 Integration with Routing Engine

The routing engine resolves a deployment's backend sequence. For each backend:

1. Check `platform_backends.provider`:
   - `anthropic` or `openai_compatible` → HTTP dispatch via `casper-proxy`.
   - `agent` → WebSocket dispatch via `AgentBackendRegistry`.
2. For agent backends: check `is_available()`. If no connection → treat as backend failure → fallback.
3. `dispatch()` → send request over WebSocket → await response with timeout.

From the caller's perspective (inference API or agent runtime), the backend type is invisible. Same response format, same error handling, same retry/fallback.

---

## 6. Sidecar Binary

### 6.1 Structure

```
casper-agent-backend/
├── src/
│   ├── main.rs           # CLI, config, startup
│   ├── ws.rs             # WebSocket client, reconnection
│   ├── dispatch.rs       # Route requests to local inference server
│   └── adapters/
│       ├── openai.rs     # vLLM, Ollama, LiteLLM (OpenAI-compatible)
│       └── tgi.rs        # HuggingFace TGI
└── config.yaml           # Sidecar configuration
```

### 6.2 Configuration

```yaml
casper_url: wss://casper.ventoo.ai/agent/connect
agent_key: csa-<key>

inference_server:
  type: openai_compatible       # openai_compatible, tgi
  base_url: http://localhost:8000
  # For vLLM/Ollama, this is the OpenAI-compatible endpoint

max_concurrent: 8               # max parallel requests to local server
```

### 6.3 Behavior

1. Connect to Casper via WebSocket with `csa-` key.
2. Send registration message.
3. Enter receive loop:
   - `ping` → respond with `pong` + load metrics.
   - `inference_request` → dispatch to local server, return response.
4. Auto-reconnect with exponential backoff on disconnect (1s, 2s, 4s, 8s, max 30s).
5. Graceful shutdown: finish active requests, then disconnect.

### 6.4 Local Dispatch

For `openai_compatible` servers:
- Translate Casper request to OpenAI Chat Completions format.
- POST to `{base_url}/v1/chat/completions`.
- For streaming: read SSE chunks, forward as `inference_chunk` messages.
- Extract usage from response (or estimate via tiktoken if not provided).

For `tgi` servers:
- Translate to TGI's generate endpoint.
- Handle TGI's different streaming format.

---

## 7. Platform Backend Configuration

An agent backend is a `platform_backend` with `provider: "agent"`:

```json
// POST /api/v1/backends
{
  "name": "ventoo-gpu-cluster",
  "provider": "agent",
  "provider_label": "Ventoo GPU (Llama 3 70B)",
  "region": "CH",
  "priority": 200,
  "max_queue_depth": 50,
  "extra_config": {
    "min_connections": 1,
    "health_check_interval_secs": 60
  }
}
```

No `base_url` or `api_key_enc` needed — the connection is inbound via WebSocket.

Then assign to models and create agent keys:

```bash
# Assign to model
POST /api/v1/models/{llama-3-70b-id}/backends
{ "backend_id": "ventoo-gpu-cluster-id", "priority": 200 }

# Create agent key
POST /api/v1/backends/{ventoo-gpu-cluster-id}/keys
{ "name": "gpu-server-01" }
→ { "key": "csa-abc123..." }  # shown once
```

A deployment can mix agent and HTTP backends:

```json
{
  "slug": "llama-3-with-fallback",
  "model_id": "...",
  "backend_sequence": [
    "ventoo-gpu-cluster-id",     // try self-hosted first (agent backend)
    "fireworks-api-id"           // fall back to commercial API (HTTP backend)
  ],
  "fallback_enabled": true
}
```

---

## 8. Observability

### 8.1 Metrics

- `agent_backend_connections` (gauge): number of connected sidecars per backend.
- `agent_backend_active_requests` (gauge): in-flight requests per backend.
- `agent_backend_request_duration` (histogram): latency distribution.
- `agent_backend_errors` (counter): by error type (timeout, overloaded, disconnected).

### 8.2 Audit

Connection and disconnection events logged:

```json
{ "action": "agent_backend.connect", "resource": "ventoo-gpu-cluster", "detail": { "hostname": "gpu-server-01" } }
{ "action": "agent_backend.disconnect", "resource": "ventoo-gpu-cluster", "detail": { "hostname": "gpu-server-01", "reason": "heartbeat_timeout" } }
```

### 8.3 Health

`GET /health` includes agent backend status:

```json
{
  "status": "ok",
  "db": "ok",
  "agent_backends": {
    "ventoo-gpu-cluster": { "connections": 2, "active_requests": 5 }
  }
}
```

---

## 9. Data Model Changes

One new table:

```sql
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
```

No changes to existing tables. The `platform_backends` table already supports `provider: "agent"`.

---

## 10. Implementation Tasks

These tasks should be executed after Phase 3 (Inference Layer) is complete.

### Task AB-1 — Agent backend key management

- `agent_backend_keys` migration.
- `POST /api/v1/backends/:id/keys` — create key, return `csa-` once.
- `GET /api/v1/backends/:id/keys` — list (prefix only).
- `DELETE /api/v1/backends/:id/keys/:key_id` — revoke.

**Verify:** Create key, verify hash stored, prefix shown in list.

---

### Task AB-2 — WebSocket endpoint and registry

- `GET /agent/connect` — WebSocket upgrade with `csa-` key auth.
- `AgentBackendRegistry`: connect, disconnect, is_available.
- Registration message handling.
- Heartbeat (ping/pong with load metrics).
- Disconnection detection (90s timeout).

**Verify:** Connect sidecar (test client), verify registered. Disconnect, verify removed. Heartbeat timeout removes stale connection.

---

### Task AB-3 — WebSocket dispatch

- `AgentBackendRegistry::dispatch()` — round-robin selection, send request, await response via oneshot, timeout handling.
- Non-streaming: send request, receive full response.
- Error handling: retryable flag, connection failures.
- Integration with routing engine: `provider: "agent"` → dispatch via registry instead of HTTP.

**Verify:** Connect test sidecar, create deployment with agent backend, send inference request, receive response. Backend failure triggers fallback to next in sequence.

---

### Task AB-4 — Streaming dispatch

- Streaming requests: `stream: true` → sidecar sends `inference_chunk` messages.
- Server reassembles chunks and forwards to caller as SSE.
- `inference_done` message signals completion with usage.

**Verify:** Streaming request via agent backend, verify chunks arrive at caller, usage tracked.

---

### Task AB-5 — Sidecar binary (OpenAI-compatible adapter)

- `casper-agent-backend` binary with config, WebSocket client, reconnection.
- Registration message on connect.
- Heartbeat response with load metrics.
- OpenAI-compatible adapter: translate request → POST to local server → translate response.
- Streaming support.

**Verify:** Start vLLM/Ollama locally, start sidecar, connect to Casper, send inference request end-to-end.

---

### Task AB-6 — Observability and health

- Prometheus metrics: connections, active requests, duration, errors.
- Audit events: connect/disconnect.
- Health endpoint includes agent backend status.

**Verify:** Connect sidecar, verify metrics. Disconnect, verify audit event. Health shows backend status.

---

## 11. Future Enhancements

- **Load-aware routing:** Use `active_requests` and `queue_depth` from pong to pick the least-loaded instance instead of round-robin.
- **Auto-scaling signals:** Expose queue depth metrics so orchestrators (Kubernetes HPA) can scale GPU servers.
- **Model hot-swap:** Sidecar reports `models_loaded`, server updates `platform_backend_models` dynamically.
- **Tenant-owned backends:** Allow tenants to connect their own GPU servers (requires scope isolation and per-tenant backend registration).
- **Batch dispatch:** Route batch/background requests preferentially to agent backends for cost optimization.

---

*End of agent backend hosting specification.*
