use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use casper_base::CasperError;
use serde_json::json;

use crate::provider::LlmProvider;
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmRole, StopReason, TokenUsage,
};

type TransportFn = dyn Fn(
        serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, CasperError>> + Send>>
    + Send
    + Sync;

pub struct LocalLlmProvider {
    transport: Arc<TransportFn>,
}

impl LocalLlmProvider {
    pub fn new<F, Fut>(transport: F) -> Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, CasperError>> + Send + 'static,
    {
        Self {
            transport: Arc::new(move |req| Box::pin(transport(req))),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for LocalLlmProvider {
    fn name(&self) -> &str {
        "local"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CasperError> {
        let start = Instant::now();
        let wire_request = build_request(&request);
        let wire_response = (self.transport)(wire_request).await?;
        parse_response(&wire_response, &request, start.elapsed())
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<ContentBlock, CasperError>> + Send>>,
        CasperError,
    > {
        let response = self.complete(request).await?;
        let mut blocks: Vec<Result<ContentBlock, CasperError>> = Vec::new();
        for block in response.reasoning {
            blocks.push(Ok(block));
        }
        for block in response.content {
            blocks.push(Ok(block));
        }
        Ok(Box::pin(futures::stream::iter(blocks)))
    }
}

// ── Request conversion ────────────────────────────────────────────

fn build_request(request: &CompletionRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = request
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                LlmRole::System => "system",
                LlmRole::User => "user",
                LlmRole::Assistant => "assistant",
                LlmRole::Tool => "tool",
            };
            json!({
                "role": role,
                "content": serde_json::to_value(&m.content).unwrap_or_default(),
            })
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
    });

    if let Some(serde_json::Value::Object(params)) = &request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in params {
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    body
}

// ── Response conversion ───────────────────────────────────────────

fn parse_response(
    resp: &serde_json::Value,
    request: &CompletionRequest,
    latency: std::time::Duration,
) -> Result<CompletionResponse, CasperError> {
    let message = resp.get("message").or_else(|| {
        resp.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
    });

    let mut content = Vec::new();

    if let Some(msg) = message {
        if let Some(text) = msg["content"].as_str()
            && !text.is_empty()
        {
            content.push(ContentBlock::Text {
                text: text.to_string(),
            });
        }

        if let Some(tool_calls) = msg["tool_calls"].as_array() {
            for tc in tool_calls {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let input: serde_json::Value = tc["function"]["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(json!({}));
                content.push(ContentBlock::ToolUse { id, name, input });
            }
        }
    } else if let Some(text) = resp["content"].as_str()
        && !text.is_empty()
    {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    let has_tool_use = content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

    let stop_reason = if has_tool_use {
        StopReason::ToolUse
    } else {
        match resp.get("stop_reason").and_then(|v| v.as_str()) {
            Some("length") => StopReason::MaxTokens,
            Some("stop_sequence") => StopReason::StopSequence,
            _ => StopReason::EndTurn,
        }
    };

    let usage = resp.get("usage");
    let input_tokens = usage
        .and_then(|u| u["input_tokens"].as_u64().or(u["prompt_tokens"].as_u64()))
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .and_then(|u| {
            u["output_tokens"]
                .as_u64()
                .or(u["completion_tokens"].as_u64())
        })
        .unwrap_or(0) as u32;

    let model = resp["model"]
        .as_str()
        .unwrap_or(request.model.as_deref().unwrap_or("local"))
        .to_string();

    Ok(CompletionResponse {
        content,
        reasoning: Vec::new(),
        stop_reason,
        usage: TokenUsage {
            input_tokens,
            output_tokens,
        },
        model,
        latency,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LlmMessage, LlmRole};

    #[test]
    fn build_request_basic() {
        let req = CompletionRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".to_string(),
                }],
            }],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.7,
            model: Some("local-model".to_string()),
            stop_sequences: vec![],
            extra: None,
        };
        let wire = build_request(&req);
        assert_eq!(wire["model"], "local-model");
        assert_eq!(wire["messages"][0]["role"], "user");
        assert_eq!(wire["max_tokens"], 1024);
    }

    #[test]
    fn parse_response_simple_text() {
        let resp = json!({
            "message": {
                "role": "assistant",
                "content": "Hello back!"
            },
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });
        let req = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.7,
            model: Some("test".to_string()),
            stop_sequences: vec![],
            extra: None,
        };
        let result = parse_response(&resp, &req, std::time::Duration::from_millis(50)).unwrap();
        assert_eq!(result.content.len(), 1);
        assert!(matches!(&result.content[0], ContentBlock::Text { text } if text == "Hello back!"));
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, 10);
        assert_eq!(result.usage.output_tokens, 5);
    }

    #[test]
    fn parse_response_with_tool_calls() {
        let resp = json!({
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"rust\"}"
                    }
                }]
            },
            "usage": { "input_tokens": 20, "output_tokens": 10 }
        });
        let req = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.7,
            model: None,
            stop_sequences: vec![],
            extra: None,
        };
        let result = parse_response(&resp, &req, std::time::Duration::ZERO).unwrap();
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(matches!(
            &result.content[0],
            ContentBlock::ToolUse { name, .. } if name == "search"
        ));
    }

    #[tokio::test]
    async fn local_provider_roundtrip() {
        let provider = LocalLlmProvider::new(|_req| async {
            Ok(json!({
                "message": { "role": "assistant", "content": "I'm local!" },
                "usage": { "input_tokens": 5, "output_tokens": 3 }
            }))
        });

        let request = CompletionRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hi".to_string(),
                }],
            }],
            tools: vec![],
            max_tokens: 512,
            temperature: 0.0,
            model: Some("local".to_string()),
            stop_sequences: vec![],
            extra: None,
        };

        let response = provider.complete(request).await.unwrap();
        assert!(
            matches!(&response.content[0], ContentBlock::Text { text } if text == "I'm local!")
        );
    }
}
