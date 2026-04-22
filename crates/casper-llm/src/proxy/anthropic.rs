use std::pin::Pin;
use std::time::{Duration, Instant};

use casper_base::CasperError;
use futures::{Stream, StreamExt};
use serde_json::json;

use crate::provider::LlmProvider;
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmMessage, LlmRole, StopReason,
    TokenUsage, ToolDefinition,
};

pub struct AnthropicProvider {
    pub client: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
}

impl AnthropicProvider {
    pub fn new(client: reqwest::Client, base_url: String, api_key: String) -> Self {
        Self {
            client,
            base_url,
            api_key,
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CasperError> {
        let start = Instant::now();
        let (system, messages) = build_messages(&request.messages);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
        });

        if let Some(system) = system {
            body["system"] = system;
        }
        if request.temperature > 0.0 {
            body["temperature"] = json!(request.temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = json!(build_tools(&request.tools));
        }
        if !request.stop_sequences.is_empty() {
            body["stop_sequences"] = json!(request.stop_sequences);
        }

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| CasperError::BadGateway(format!("Anthropic request failed: {e}")))?;

        let status = response.status();
        let text = response.text().await.map_err(|e| {
            CasperError::BadGateway(format!("Failed to read Anthropic response: {e}"))
        })?;

        if !status.is_success() {
            return Err(map_anthropic_error(status.as_u16(), &text));
        }

        let resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| CasperError::BadGateway(format!("Invalid Anthropic JSON: {e}")))?;

        parse_response(&resp, start.elapsed())
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>
    {
        let (system, messages) = build_messages(&request.messages);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        if let Some(system) = system {
            body["system"] = system;
        }
        if request.temperature > 0.0 {
            body["temperature"] = json!(request.temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = json!(build_tools(&request.tools));
        }
        if !request.stop_sequences.is_empty() {
            body["stop_sequences"] = json!(request.stop_sequences);
        }

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(600))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CasperError::BadGateway(format!("Anthropic stream request failed: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_anthropic_error(status.as_u16(), &text));
        }

        let byte_stream = response.bytes_stream();

        let stream = futures::stream::unfold(
            SseState {
                byte_stream: Box::pin(byte_stream),
                buffer: String::new(),
                current_tool_id: String::new(),
                current_tool_name: String::new(),
                current_tool_json: String::new(),
                current_block_type: None,
                done: false,
            },
            |mut state| async move {
                if state.done {
                    return None;
                }

                loop {
                    // Try to extract a complete SSE event from buffer
                    if let Some(block) = state.try_parse_event() {
                        return Some((block, state));
                    }

                    // Read more data
                    match state.byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            let chunk = String::from_utf8_lossy(&bytes)
                                .replace("\r\n", "\n")
                                .replace('\r', "\n");
                            state.buffer.push_str(&chunk);
                        }
                        Some(Err(e)) => {
                            state.done = true;
                            return Some((
                                Err(CasperError::BadGateway(format!("Stream read error: {e}"))),
                                state,
                            ));
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

struct SseState {
    byte_stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: String,
    current_tool_id: String,
    current_tool_name: String,
    current_tool_json: String,
    current_block_type: Option<String>,
    done: bool,
}

impl SseState {
    fn try_parse_event(&mut self) -> Option<Result<ContentBlock, CasperError>> {
        while let Some(pos) = self.buffer.find("\n\n") {
            let event_block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            let mut event_type = String::new();
            let mut data_lines: Vec<String> = Vec::new();
            for line in event_block.lines() {
                if let Some(et) = line.strip_prefix("event: ") {
                    event_type = et.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data_lines.push(d.to_string());
                }
            }

            let data_str = data_lines.join("\n");
            if data_str.is_empty() {
                continue;
            }
            let data: serde_json::Value = match serde_json::from_str(&data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            match event_type.as_str() {
                "content_block_start" => {
                    let block = &data["content_block"];
                    let btype = block["type"].as_str().unwrap_or("");
                    self.current_block_type = Some(btype.to_string());
                    if btype == "tool_use" {
                        self.current_tool_id = block["id"].as_str().unwrap_or("").to_string();
                        self.current_tool_name = block["name"].as_str().unwrap_or("").to_string();
                        self.current_tool_json.clear();
                    }
                }
                "content_block_delta" => {
                    let delta = &data["delta"];
                    match delta["type"].as_str() {
                        Some("thinking_delta") => {
                            if let Some(t) = delta["thinking"].as_str() {
                                return Some(Ok(ContentBlock::Thinking {
                                    text: t.to_string(),
                                }));
                            }
                        }
                        Some("text_delta") => {
                            if let Some(t) = delta["text"].as_str() {
                                return Some(Ok(ContentBlock::Text {
                                    text: t.to_string(),
                                }));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(j) = delta["partial_json"].as_str() {
                                self.current_tool_json.push_str(j);
                            }
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    if self.current_block_type.as_deref() == Some("tool_use") {
                        let input: serde_json::Value =
                            serde_json::from_str(&self.current_tool_json).unwrap_or(json!({}));
                        self.current_block_type = None;
                        return Some(Ok(ContentBlock::ToolUse {
                            id: std::mem::take(&mut self.current_tool_id),
                            name: std::mem::take(&mut self.current_tool_name),
                            input,
                        }));
                    }
                    self.current_block_type = None;
                }
                "message_stop" => {
                    self.done = true;
                    return None;
                }
                _ => {}
            }
        }
        None
    }
}

// ── Request conversion ────────────────────────────────────────────

fn build_messages(messages: &[LlmMessage]) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system: Option<serde_json::Value> = None;
    let mut out = Vec::new();

    for msg in messages {
        match msg.role {
            LlmRole::System => {
                let text = msg
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                system = Some(json!(text));
            }
            LlmRole::Assistant => {
                let blocks = msg
                    .content
                    .iter()
                    .map(content_block_to_anthropic)
                    .collect::<Vec<_>>();
                out.push(json!({"role": "assistant", "content": blocks}));
            }
            LlmRole::Tool => {
                let blocks: Vec<serde_json::Value> = msg
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => Some(json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content,
                            "is_error": is_error,
                        })),
                        _ => None,
                    })
                    .collect();
                out.push(json!({"role": "user", "content": blocks}));
            }
            LlmRole::User => {
                let blocks = msg
                    .content
                    .iter()
                    .map(content_block_to_anthropic)
                    .collect::<Vec<_>>();
                out.push(json!({"role": "user", "content": blocks}));
            }
        }
    }

    (system, out)
}

fn content_block_to_anthropic(block: &ContentBlock) -> serde_json::Value {
    match block {
        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
        ContentBlock::Image { media_type, source } => {
            let mt = match media_type {
                crate::types::ImageMediaType::Png => "image/png",
                crate::types::ImageMediaType::Jpeg => "image/jpeg",
                crate::types::ImageMediaType::Gif => "image/gif",
                crate::types::ImageMediaType::Webp => "image/webp",
            };
            match source {
                crate::types::ImageSource::Base64 { data } => json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": mt, "data": data}
                }),
                crate::types::ImageSource::Url { url } => json!({
                    "type": "image",
                    "source": {"type": "url", "url": url}
                }),
            }
        }
        ContentBlock::Thinking { text } => json!({"type": "thinking", "thinking": text}),
        ContentBlock::ToolUse { id, name, input } => json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content,
            "is_error": is_error,
        }),
    }
}

fn build_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect()
}

// ── Response conversion ───────────────────────────────────────────

fn parse_response(
    resp: &serde_json::Value,
    latency: Duration,
) -> Result<CompletionResponse, CasperError> {
    let mut content = Vec::new();
    let mut reasoning = Vec::new();

    if let Some(blocks) = resp["content"].as_array() {
        for block in blocks {
            match block["type"].as_str() {
                Some("thinking") => {
                    if let Some(text) = block["thinking"].as_str() {
                        reasoning.push(ContentBlock::Thinking {
                            text: text.to_string(),
                        });
                    }
                }
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        content.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
                Some("tool_use") => {
                    content.push(ContentBlock::ToolUse {
                        id: block["id"].as_str().unwrap_or("").to_string(),
                        name: block["name"].as_str().unwrap_or("").to_string(),
                        input: block.get("input").cloned().unwrap_or(json!({})),
                    });
                }
                _ => {}
            }
        }
    }

    let usage_obj = &resp["usage"];
    let input_tokens = usage_obj["input_tokens"].as_u64().unwrap_or(0) as u32;
    let output_tokens = usage_obj["output_tokens"].as_u64().unwrap_or(0) as u32;

    let stop_reason = match resp["stop_reason"].as_str() {
        Some("end_turn") => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("stop_sequence") => StopReason::StopSequence,
        _ => StopReason::EndTurn,
    };

    let model = resp["model"].as_str().unwrap_or("unknown").to_string();

    Ok(CompletionResponse {
        content,
        reasoning,
        stop_reason,
        usage: TokenUsage {
            input_tokens,
            output_tokens,
        },
        model,
        latency,
    })
}

fn map_anthropic_error(status: u16, body: &str) -> CasperError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| body.chars().take(500).collect());

    match status {
        401 => CasperError::BadGateway(format!("Anthropic auth error: {message}")),
        429 => CasperError::RateLimited,
        500..=599 => {
            CasperError::BadGateway(format!("Anthropic server error ({status}): {message}"))
        }
        _ => CasperError::BadGateway(format!("Anthropic error ({status}): {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_extracted_from_messages() {
        let messages = vec![
            LlmMessage {
                role: LlmRole::System,
                content: vec![ContentBlock::Text {
                    text: "You are a helpful assistant".to_string(),
                }],
            },
            LlmMessage {
                role: LlmRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".to_string(),
                }],
            },
        ];

        let (system, msgs) = build_messages(&messages);
        assert_eq!(system, Some(json!("You are a helpful assistant")));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn no_system() {
        let messages = vec![LlmMessage {
            role: LlmRole::User,
            content: vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
        }];

        let (system, msgs) = build_messages(&messages);
        assert!(system.is_none());
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn tool_use_converted() {
        let messages = vec![LlmMessage {
            role: LlmRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me search.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "web_search".to_string(),
                    input: json!({"query": "rust"}),
                },
            ],
        }];

        let (_, msgs) = build_messages(&messages);
        assert_eq!(msgs.len(), 1);
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["name"], "web_search");
    }

    #[test]
    fn tool_result_converted() {
        let messages = vec![LlmMessage {
            role: LlmRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "search results here".to_string(),
                is_error: false,
            }],
        }];

        let (_, msgs) = build_messages(&messages);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "call_1");
    }

    #[test]
    fn parse_anthropic_response() {
        let resp = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Hello! How can I help?"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 25,
                "output_tokens": 10,
            }
        });

        let result = parse_response(&resp, Duration::from_millis(100)).unwrap();
        assert_eq!(result.content.len(), 1);
        assert!(
            matches!(&result.content[0], ContentBlock::Text { text } if text == "Hello! How can I help?")
        );
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, 25);
        assert_eq!(result.usage.output_tokens, 10);
    }

    #[test]
    fn parse_anthropic_tool_use() {
        let resp = json!({
            "id": "msg_456",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Let me search."},
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "search",
                    "input": {"query": "rust programming"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 50, "output_tokens": 30}
        });

        let result = parse_response(&resp, Duration::from_millis(200)).unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(matches!(
            &result.content[1],
            ContentBlock::ToolUse { name, .. } if name == "search"
        ));
    }

    #[test]
    fn parse_anthropic_with_thinking() {
        let resp = json!({
            "id": "msg_789",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "thinking", "thinking": "Let me think about this..."},
                {"type": "text", "text": "The answer is 42."}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = parse_response(&resp, Duration::from_millis(50)).unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.reasoning.len(), 1);
        assert!(
            matches!(&result.reasoning[0], ContentBlock::Thinking { text } if text == "Let me think about this...")
        );
    }

    #[test]
    fn error_mapping() {
        let err = map_anthropic_error(429, r#"{"error":{"message":"rate limited"}}"#);
        assert!(matches!(err, CasperError::RateLimited));

        let err = map_anthropic_error(500, r#"{"error":{"message":"internal"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));

        let err = map_anthropic_error(401, r#"{"error":{"message":"invalid key"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));
    }

    #[test]
    fn build_tool_definitions() {
        let tools = vec![ToolDefinition {
            name: "get_weather".to_string(),
            description: "Get weather".to_string(),
            input_schema: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        }];
        let result = build_tools(&tools);
        assert_eq!(result[0]["name"], "get_weather");
        assert_eq!(
            result[0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
    }
}
