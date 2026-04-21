//! Mock LLM caller for testing.
//!
//! The `LlmCaller` trait is defined in `casper-catalog`. The real
//! implementation lives in `casper-server` where concrete infrastructure
//! (HTTP clients, WebSocket registries) is available.

#[cfg(test)]
use casper_base::CasperError;
#[cfg(test)]
use casper_catalog::{LlmCaller, LlmResponse, MessageRole};
#[cfg(test)]
use uuid::Uuid;

#[cfg(test)]
use casper_catalog::LlmRequest;
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
pub struct MockLlmCaller {
    responses: std::sync::Mutex<Vec<LlmResponse>>,
}

#[cfg(test)]
impl MockLlmCaller {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
        }
    }

    pub fn simple(text: &str) -> Self {
        Self::new(vec![LlmResponse {
            content: text.to_string(),
            role: MessageRole::Assistant,
            model: "mock-model".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            tool_calls: None,
            finish_reason: Some("end_turn".to_string()),
            thinking: None,
        }])
    }

    pub fn with_tool_call(
        tool_name: &str,
        tool_input: serde_json::Value,
        final_text: &str,
    ) -> Self {
        Self::new(vec![
            LlmResponse {
                content: String::new(),
                role: MessageRole::Assistant,
                model: "mock-model".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                tool_calls: Some(vec![json!({
                    "type": "tool_use",
                    "id": "call_001",
                    "name": tool_name,
                    "input": tool_input,
                })]),
                finish_reason: Some("tool_use".to_string()),
                thinking: None,
            },
            LlmResponse {
                content: final_text.to_string(),
                role: MessageRole::Assistant,
                model: "mock-model".to_string(),
                input_tokens: 150,
                output_tokens: 60,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                tool_calls: None,
                finish_reason: Some("end_turn".to_string()),
                thinking: None,
            },
        ])
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl LlmCaller for MockLlmCaller {
    async fn call(
        &self,
        _tenant_id: Uuid,
        _request: &LlmRequest,
    ) -> Result<(LlmResponse, Option<Uuid>), CasperError> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(CasperError::Internal(
                "MockLlmCaller: no more responses".into(),
            ));
        }
        Ok((responses.remove(0), None))
    }
}
