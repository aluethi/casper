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
        let mut tool_acc: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            // Streaming text delta
            if let Some(delta) = chunk["delta"].as_str() {
                if !delta.is_empty() {
                    content.push(ContentBlock::Text {
                        text: delta.to_string(),
                    });
                }
                continue;
            }
            // Streaming thinking delta
            if let Some(delta) = chunk["thinking_delta"].as_str() {
                if !delta.is_empty() {
                    reasoning.push(ContentBlock::Thinking {
                        text: delta.to_string(),
                    });
                }
                continue;
            }
            // Streaming tool call start
            if let Some(tc) = chunk.get("tool_call_start") {
                let idx = tc["tool_index"].as_u64().unwrap_or(0) as usize;
                let id = tc["tool_call_id"].as_str().unwrap_or("").to_string();
                let name = tc["name"].as_str().unwrap_or("").to_string();
                tool_acc.insert(idx, (id, name, String::new()));
                continue;
            }
            // Streaming tool call argument delta
            if let Some(td) = chunk.get("tool_call_delta") {
                let idx = td["tool_index"].as_u64().unwrap_or(0) as usize;
                if let Some(entry) = tool_acc.get_mut(&idx)
                    && let Some(args) = td["arguments_delta"].as_str()
                {
                    entry.2.push_str(args);
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

        // Finalize accumulated tool calls
        let mut indices: Vec<usize> = tool_acc.keys().copied().collect();
        indices.sort();
        for idx in indices {
            let (id, name, args) = tool_acc.remove(&idx).unwrap();
            let input: serde_json::Value = serde_json::from_str(&args).unwrap_or(json!({}));
            content.push(ContentBlock::ToolUse { id, name, input });
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

        let block_stream = futures::stream::unfold(
            StreamState {
                inner: Box::pin(json_stream),
                tool_acc: std::collections::HashMap::new(),
                pending: Vec::new(),
                done: false,
            },
            |mut state| async move {
                // Drain pending blocks first (from message fallback or finalized tools)
                if let Some(block) = state.pending.pop() {
                    return Some((Ok(block), state));
                }
                if state.done {
                    return None;
                }

                loop {
                    match state.inner.next().await {
                        Some(Err(e)) => {
                            state.done = true;
                            return Some((Err(e), state));
                        }
                        Some(Ok(chunk)) => {
                            // Text delta
                            if let Some(delta) = chunk["delta"].as_str() {
                                if !delta.is_empty() {
                                    return Some((
                                        Ok(ContentBlock::Text {
                                            text: delta.to_string(),
                                        }),
                                        state,
                                    ));
                                }
                                continue;
                            }
                            // Thinking delta
                            if let Some(delta) = chunk["thinking_delta"].as_str() {
                                if !delta.is_empty() {
                                    return Some((
                                        Ok(ContentBlock::Thinking {
                                            text: delta.to_string(),
                                        }),
                                        state,
                                    ));
                                }
                                continue;
                            }
                            // Tool call start — begin accumulating arguments
                            if let Some(tc) = chunk.get("tool_call_start") {
                                let idx = tc["tool_index"].as_u64().unwrap_or(0) as usize;
                                let id = tc["tool_call_id"].as_str().unwrap_or("").to_string();
                                let name = tc["name"].as_str().unwrap_or("").to_string();
                                state.tool_acc.insert(idx, (id, name, String::new()));
                                continue;
                            }
                            // Tool call argument delta
                            if let Some(td) = chunk.get("tool_call_delta") {
                                let idx = td["tool_index"].as_u64().unwrap_or(0) as usize;
                                if let Some(entry) = state.tool_acc.get_mut(&idx)
                                    && let Some(args) = td["arguments_delta"].as_str()
                                {
                                    entry.2.push_str(args);
                                }
                                continue;
                            }
                            // Full response fallback — emit all blocks
                            if let Some(msg) = chunk.get("message") {
                                let mut blocks = Vec::new();
                                parse_message_into(msg, &mut blocks, &mut Vec::new());
                                // Reverse so pop() yields them in order
                                blocks.reverse();
                                state.pending = blocks;
                                if let Some(block) = state.pending.pop() {
                                    return Some((Ok(block), state));
                                }
                                continue;
                            }
                            // Done — finalize any accumulated tool calls
                            if chunk.get("done").is_some() {
                                state.finalize_tools();
                                state.done = true;
                                if let Some(block) = state.pending.pop() {
                                    return Some((Ok(block), state));
                                }
                                return None;
                            }
                            continue;
                        }
                        None => {
                            // Stream ended — finalize any remaining tool calls
                            state.finalize_tools();
                            state.done = true;
                            if let Some(block) = state.pending.pop() {
                                return Some((Ok(block), state));
                            }
                            return None;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(block_stream))
    }
}

struct StreamState {
    inner: Pin<Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>>,
    tool_acc: std::collections::HashMap<usize, (String, String, String)>,
    pending: Vec<ContentBlock>,
    done: bool,
}

impl StreamState {
    fn finalize_tools(&mut self) {
        if self.tool_acc.is_empty() {
            return;
        }
        let mut indices: Vec<usize> = self.tool_acc.keys().copied().collect();
        indices.sort();
        // Reverse so pop() yields them in order
        indices.reverse();
        for idx in indices {
            let (id, name, args) = self.tool_acc.remove(&idx).unwrap();
            let input: serde_json::Value = serde_json::from_str(&args).unwrap_or(json!({}));
            self.pending.push(ContentBlock::ToolUse { id, name, input });
        }
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

    if !request.tools.is_empty() {
        let tools: Vec<serde_json::Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
        body["tools"] = json!(tools);
    }

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

    #[tokio::test]
    async fn streaming_thinking_deltas() {
        let provider = LocalLlmProvider::new(|_req| async {
            let chunks = vec![
                Ok(json!({ "thinking_delta": "Let me " })),
                Ok(json!({ "thinking_delta": "think..." })),
                Ok(json!({ "delta": "The answer is 42." })),
                Ok(json!({ "done": true })),
            ];
            Ok(Box::pin(futures::stream::iter(chunks))
                as Pin<
                    Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>,
                >)
        });

        let request = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 512,
            temperature: 0.0,
            model: Some("local".to_string()),
            stop_sequences: vec![],
            extra: None,
        };

        let mut stream = provider.complete_stream(request).await.unwrap();
        let mut thinking = Vec::new();
        let mut content = Vec::new();
        while let Some(Ok(block)) = stream.next().await {
            match block {
                ContentBlock::Thinking { text } => thinking.push(text),
                ContentBlock::Text { text } => content.push(text),
                _ => {}
            }
        }
        assert_eq!(thinking, vec!["Let me ", "think..."]);
        assert_eq!(content, vec!["The answer is 42."]);
    }

    #[tokio::test]
    async fn streaming_tool_calls() {
        let provider = LocalLlmProvider::new(|_req| async {
            let chunks = vec![
                Ok(json!({ "delta": "Let me search." })),
                Ok(
                    json!({ "tool_call_start": { "tool_index": 0, "tool_call_id": "call_1", "name": "web_search" } }),
                ),
                Ok(
                    json!({ "tool_call_delta": { "tool_index": 0, "arguments_delta": "{\"query\":" } }),
                ),
                Ok(
                    json!({ "tool_call_delta": { "tool_index": 0, "arguments_delta": "\"rust\"}" } }),
                ),
                Ok(json!({ "done": true })),
            ];
            Ok(Box::pin(futures::stream::iter(chunks))
                as Pin<
                    Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>,
                >)
        });

        let request = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 512,
            temperature: 0.0,
            model: Some("local".to_string()),
            stop_sequences: vec![],
            extra: None,
        };

        // Test streaming
        let mut stream = provider.complete_stream(request.clone()).await.unwrap();
        let mut blocks = Vec::new();
        while let Some(Ok(block)) = stream.next().await {
            blocks.push(block);
        }
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "Let me search."));
        assert!(
            matches!(&blocks[1], ContentBlock::ToolUse { name, input, .. } if name == "web_search" && input["query"] == "rust")
        );

        // Test complete (accumulates same blocks)
        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.content.len(), 2);
        assert!(
            matches!(&response.content[1], ContentBlock::ToolUse { name, .. } if name == "web_search")
        );
        assert_eq!(response.stop_reason, StopReason::ToolUse);
    }

    #[tokio::test]
    async fn full_response_with_tool_calls() {
        let provider = LocalLlmProvider::new(|_req| async {
            let chunks = vec![Ok(json!({
                "message": {
                    "role": "assistant",
                    "content": "Let me check.",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "get_weather", "arguments": "{\"city\":\"Zurich\"}" }
                    }]
                },
                "stop_reason": "tool_calls",
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
            temperature: 0.0,
            model: Some("test".to_string()),
            stop_sequences: vec![],
            extra: None,
        };

        // complete() should return both text and tool_use blocks
        let response = provider.complete(request.clone()).await.unwrap();
        assert_eq!(response.content.len(), 2);
        assert!(
            matches!(&response.content[0], ContentBlock::Text { text } if text == "Let me check.")
        );
        assert!(
            matches!(&response.content[1], ContentBlock::ToolUse { name, .. } if name == "get_weather")
        );
        assert_eq!(response.stop_reason, StopReason::ToolUse);

        // complete_stream() should also emit both blocks
        let mut stream = provider.complete_stream(request).await.unwrap();
        let mut blocks = Vec::new();
        while let Some(Ok(block)) = stream.next().await {
            blocks.push(block);
        }
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "Let me check."));
        assert!(matches!(&blocks[1], ContentBlock::ToolUse { name, .. } if name == "get_weather"));
    }
}
