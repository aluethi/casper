use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Messages exchanged over the agent backend WebSocket connection.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "register")]
    Register {
        backend_id: Uuid,
        hostname: String,
        #[serde(default)]
        inference_server: Option<String>,
        #[serde(default)]
        inference_version: Option<String>,
        #[serde(default)]
        models_loaded: Vec<String>,
        #[serde(default = "default_max_concurrent")]
        max_concurrent: u32,
        #[serde(default)]
        gpu_info: Option<serde_json::Value>,
    },
    #[serde(rename = "ping")]
    Ping { timestamp: String },
    #[serde(rename = "pong")]
    Pong {
        timestamp: String,
        #[serde(default)]
        active_requests: u32,
        #[serde(default)]
        queue_depth: u32,
    },
    #[serde(rename = "inference_request")]
    InferenceRequest {
        id: String,
        model: String,
        messages: Vec<serde_json::Value>,
        params: serde_json::Value,
        timeout_ms: u64,
    },
    #[serde(rename = "inference_response")]
    InferenceResponse {
        id: String,
        status: String,
        message: Option<InferenceResponseMessage>,
        usage: Option<InferenceUsage>,
        duration_ms: Option<u64>,
        #[serde(default)]
        stop_reason: Option<String>,
    },
    #[serde(rename = "inference_error")]
    InferenceError {
        id: String,
        error: String,
        message: Option<String>,
        #[serde(default)]
        retryable: bool,
    },
    #[serde(rename = "register_ack")]
    RegisterAck {
        status: String,
        backend_id: uuid::Uuid,
        #[serde(default)]
        config: RegisterAckConfig,
    },
}

/// Server-side config sent to the sidecar on registration.
/// Only includes platform-managed settings (concurrency, identity).
/// Inference URLs are a sidecar-local concern — the server doesn't know them.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RegisterAckConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(default)]
    pub hostname: Option<String>,
}

fn default_max_concurrent() -> u32 {
    8
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceResponseMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InferenceUsage {
    #[serde(default)]
    pub input_tokens: i32,
    #[serde(default)]
    pub output_tokens: i32,
    #[serde(default)]
    pub cache_read_tokens: Option<i32>,
    #[serde(default)]
    pub cache_write_tokens: Option<i32>,
}
