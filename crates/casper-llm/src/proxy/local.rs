use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use casper_base::CasperError;
use futures::{Stream, StreamExt};
use serde_json::json;

use crate::provider::LlmProvider;
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmRole, StopReason, TokenUsage,
};

type TransportFn = dyn Fn(
        serde_json::Value,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Pin<Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>>,
                        CasperError,
                    >,
                > + Send,
        >,
    > + Send
    + Sync;

pub struct LocalLlmProvider {
    transport: Arc<TransportFn>,
}

impl LocalLlmProvider {
    pub fn new<F, Fut>(transport: F) -> Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<
                Output = Result<
                    Pin<Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>>,
                    CasperError,
                >,
            > + Send
            + 'static,
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
        let mut stream = (self.transport)(wire_request).await?;

        let mut content = Vec::new();
        let mut reasoning = Vec::new();
        let mut usage = TokenUsage::default();
        let mut stop_reason = None;
        let mut model = request.model.clone().unwrap_or_else(|| "local".to_string());

        while let Some(item) = stream.next().await {
            let chunk = item?;
            // Streaming delta
            if let Some(delta) = chunk["delta"].as_str() {
                if !delta.is_empty() {
                    content.push(ContentBlock::Text {
                        text: delta.to_string(),
                    });
                }
                continue;
            }
            // Full response (non-streaming fallback)
            if let Some(msg) = chunk.get("message") {
                parse_message_into(msg, &mut content, &mut reasoning);
                if let Some(sr) = chunk.get("stop_reason").and_then(|v| v.as_str()) {
                    stop_reason = Some(parse_stop_reason(sr));
                }
            }
            // Done / usage
            if let Some(u) = chunk.get("usage") {
                usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0) as u32;
                usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0) as u32;
            }
            if let Some(m) = chunk["model"].as_str() {
                model = m.to_string();
            }
        }

        let has_tool_use = content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

        Ok(CompletionResponse {
            content,
            reasoning,
            stop_reason: stop_reason.unwrap_or(if has_tool_use {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            }),
            usage,
            model,
            latency: start.elapsed(),
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>
    {
        let wire_request = build_request(&request);
        let json_stream = (self.transport)(wire_request).await?;

        let block_stream = json_stream.filter_map(|item| async move {
            match item {
                Err(e) => Some(Err(e)),
                Ok(chunk) => {
                    // Streaming text delta
                    if let Some(delta) = chunk["delta"].as_str() {
                        if !delta.is_empty() {
                            return Some(Ok(ContentBlock::Text {
                                text: delta.to_string(),
                            }));
                        }
                        return None;
                    }
                    // Full response fallback — emit each block
                    if let Some(msg) = chunk.get("message") {
                        let mut blocks = Vec::new();
                        parse_message_into(msg, &mut blocks, &mut Vec::new());
                        if let Some(first) = blocks.into_iter().next() {
                            return Some(Ok(first));
                        }
                    }
                    None
                }
            }
        });

        Ok(Box::pin(block_stream))
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

// ── Response parsing helpers ──────────────────────────────────────

fn parse_message_into(
    msg: &serde_json::Value,
    content: &mut Vec<ContentBlock>,
    _reasoning: &mut Vec<ContentBlock>,
) {
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
}

fn parse_stop_reason(s: &str) -> StopReason {
    match s {
        "length" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "tool_calls" | "tool_use" => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    }
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

    #[tokio::test]
    async fn local_provider_streaming() {
        let provider = LocalLlmProvider::new(|_req| async {
            let chunks = vec![
                Ok(json!({ "delta": "Hello " })),
                Ok(json!({ "delta": "world!" })),
                Ok(json!({ "done": true, "usage": { "input_tokens": 5, "output_tokens": 2 } })),
            ];
            Ok(Box::pin(futures::stream::iter(chunks))
                as Pin<
                    Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>,
                >)
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

        // Test streaming
        let mut stream = provider.complete_stream(request.clone()).await.unwrap();
        let mut texts = Vec::new();
        while let Some(Ok(block)) = stream.next().await {
            if let ContentBlock::Text { text } = block {
                texts.push(text);
            }
        }
        assert_eq!(texts, vec!["Hello ", "world!"]);

        // Test complete (accumulates)
        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.content.len(), 2);
        assert_eq!(response.usage.input_tokens, 5);
    }

    #[tokio::test]
    async fn local_provider_full_response_fallback() {
        let provider = LocalLlmProvider::new(|_req| async {
            let chunks = vec![Ok(json!({
                "message": { "role": "assistant", "content": "Full response" },
                "usage": { "input_tokens": 10, "output_tokens": 5 }
            }))];
            Ok(Box::pin(futures::stream::iter(chunks))
                as Pin<
                    Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>,
                >)
        });

        let request = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.7,
            model: Some("test".to_string()),
            stop_sequences: vec![],
            extra: None,
        };

        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.content.len(), 1);
        assert!(
            matches!(&response.content[0], ContentBlock::Text { text } if text == "Full response")
        );
    }
}
