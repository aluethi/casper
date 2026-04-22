//! Shared WebSocket protocol for Casper agent backends.
//!
//! This crate is the single source of truth for messages exchanged between
//! the casper-server and the casper-agent-backend sidecar. Both depend on
//! this crate — protocol mismatches are caught at compile time.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Top-level message envelope ───────────────────────────────────

/// All messages on the agent backend WebSocket are tagged JSON.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Sidecar → Server: register after connect.
    Register(Register),

    /// Server → Sidecar: acknowledge registration with config.
    RegisterAck(RegisterAck),

    /// Server → Sidecar: heartbeat ping.
    Ping { timestamp: String },

    /// Sidecar → Server: heartbeat response with load metrics.
    Pong(Pong),

    /// Server → Sidecar: run an inference request.
    InferenceRequest(InferenceRequest),

    /// Sidecar → Server: full (non-streaming) response.
    InferenceResponse(InferenceResponse),

    /// Sidecar → Server: error response.
    InferenceError(InferenceError),

    /// Sidecar → Server: streaming chunk.
    InferenceChunk { id: String, delta: String },

    /// Sidecar → Server: streaming done with usage.
    InferenceDone(InferenceDone),
}

// ── Individual message types ─────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Register {
    /// Hostname of the sidecar machine (informational).
    pub hostname: String,
    /// GPU info (optional, informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_info: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RegisterAck {
    pub status: String,
    /// The backend ID this key is bound to (server tells the sidecar).
    pub backend_id: Uuid,
    /// Platform-managed config pushed to the sidecar.
    #[serde(default)]
    pub config: RegisterAckConfig,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RegisterAckConfig {
    /// Max concurrent inference requests the server will send.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
}

fn default_max_concurrent() -> u32 {
    8
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pong {
    pub timestamp: String,
    #[serde(default)]
    pub active_requests: u32,
    #[serde(default)]
    pub queue_depth: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceRequest {
    pub id: String,
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub params: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceResponse {
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<InferenceMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<InferenceUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceError {
    pub id: String,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceDone {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<InferenceUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct InferenceUsage {
    #[serde(default)]
    pub input_tokens: i32,
    #[serde(default)]
    pub output_tokens: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_roundtrip() {
        let msg = WsMessage::Register(Register {
            hostname: "gpu-01".into(),
            gpu_info: None,
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"register\""));
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, WsMessage::Register(_)));
    }

    #[test]
    fn register_ack_roundtrip() {
        let msg = WsMessage::RegisterAck(RegisterAck {
            status: "ok".into(),
            backend_id: Uuid::nil(),
            config: RegisterAckConfig::default(),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, WsMessage::RegisterAck(_)));
    }

    #[test]
    fn inference_response_roundtrip() {
        let msg = WsMessage::InferenceResponse(InferenceResponse {
            id: "req-1".into(),
            status: "ok".into(),
            message: Some(InferenceMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            }),
            usage: Some(InferenceUsage {
                input_tokens: 10,
                output_tokens: 3,
                ..Default::default()
            }),
            duration_ms: Some(150),
            stop_reason: Some("stop".into()),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, WsMessage::InferenceResponse(_)));
    }
}
