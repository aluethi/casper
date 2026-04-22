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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenAiVariant {
    Standard,
    Azure,
}

pub struct OpenAiProvider {
    pub client: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
    pub variant: OpenAiVariant,
}

impl OpenAiProvider {
    pub fn standard(client: reqwest::Client, base_url: String, api_key: String) -> Self {
        Self {
            client,
            base_url,
            api_key,
            variant: OpenAiVariant::Standard,
        }
    }

    pub fn azure(client: reqwest::Client, base_url: String, api_key: String) -> Self {
        Self {
            client,
            base_url,
            api_key,
            variant: OpenAiVariant::Azure,
        }
    }

    fn url(&self) -> String {
        match self.variant {
            OpenAiVariant::Azure => self.base_url.clone(),
            OpenAiVariant::Standard => {
                let base = self.base_url.trim_end_matches('/');
                if base.ends_with("/v1") {
                    format!("{base}/chat/completions")
                } else {
                    format!("{base}/v1/chat/completions")
                }
            }
        }
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.variant {
            OpenAiVariant::Azure => req.header("api-key", &self.api_key),
            OpenAiVariant::Standard => {
                req.header("authorization", format!("Bearer {}", self.api_key))
            }
        }
    }

    fn max_tokens_key(&self) -> &'static str {
        match self.variant {
            OpenAiVariant::Azure => "max_completion_tokens",
            OpenAiVariant::Standard => "max_tokens",
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        match self.variant {
            OpenAiVariant::Standard => "openai",
            OpenAiVariant::Azure => "azure_openai",
        }
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CasperError> {
        let start = Instant::now();
        let messages = build_messages(&request.messages);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
        });

        body[self.max_tokens_key()] = json!(request.max_tokens);

        if request.temperature > 0.0 {
            body["temperature"] = json!(request.temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = json!(build_tools(&request.tools));
        }
        if !request.stop_sequences.is_empty() {
            body["stop"] = json!(request.stop_sequences);
        }

        let url = self.url();
        let http_req = self
            .apply_auth(self.client.post(&url))
            .header("content-type", "application/json");

        let response = http_req
            .json(&body)
            .send()
            .await
            .map_err(|e| CasperError::BadGateway(format!("OpenAI request failed: {e}")))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| CasperError::BadGateway(format!("Failed to read OpenAI response: {e}")))?;

        if !status.is_success() {
            return Err(map_openai_error(status.as_u16(), &text));
        }

        let resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| CasperError::BadGateway(format!("Invalid OpenAI JSON: {e}")))?;

        parse_response(&resp, start.elapsed())
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>
    {
        let messages = build_messages(&request.messages);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            "stream_options": {"include_usage": true},
        });

        body[self.max_tokens_key()] = json!(request.max_tokens);

        if request.temperature > 0.0 {
            body["temperature"] = json!(request.temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = json!(build_tools(&request.tools));
        }
        if !request.stop_sequences.is_empty() {
            body["stop"] = json!(request.stop_sequences);
        }

        let url = self.url();
        let http_req = self
            .apply_auth(self.client.post(&url))
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(600));

        let response =
            http_req.json(&body).send().await.map_err(|e| {
                CasperError::BadGateway(format!("OpenAI stream request failed: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_openai_error(status.as_u16(), &text));
        }

        let byte_stream = response.bytes_stream();

        let stream = futures::stream::unfold(
            OpenAiSseState {
                byte_stream: Box::pin(byte_stream),
                buffer: String::new(),
                tool_acc: std::collections::HashMap::new(),
                pending_blocks: Vec::new(),
                done: false,
            },
            |mut state| async move {
                // Drain pending blocks first (accumulated tool calls emitted at end)
                if let Some(block) = state.pending_blocks.pop() {
                    return Some((Ok(block), state));
                }

                if state.done {
                    return None;
                }

                loop {
                    if let Some(block) = state.try_parse_line() {
                        return Some((block, state));
                    }

                    // Drain any pending blocks generated by finalization
                    if let Some(block) = state.pending_blocks.pop() {
                        return Some((Ok(block), state));
                    }

                    if state.done {
                        return None;
                    }

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
                        None => {
                            state.finalize_tools();
                            if let Some(block) = state.pending_blocks.pop() {
                                state.done = true;
                                return Some((Ok(block), state));
                            }
                            return None;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

struct OpenAiSseState {
    byte_stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: String,
    tool_acc: std::collections::HashMap<usize, (String, String, String)>,
    pending_blocks: Vec<ContentBlock>,
    done: bool,
}

impl OpenAiSseState {
    fn try_parse_line(&mut self) -> Option<Result<ContentBlock, CasperError>> {
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 1..].to_string();

            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let data_str = match line.strip_prefix("data: ") {
                Some(d) => d,
                None => continue,
            };
            if data_str == "[DONE]" {
                self.finalize_tools();
                self.done = true;
                if let Some(block) = self.pending_blocks.pop() {
                    return Some(Ok(block));
                }
                return None;
            }

            let data: serde_json::Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(choice) = data["choices"].as_array().and_then(|a| a.first()) {
                let delta = &choice["delta"];

                if let Some(c) = delta["content"].as_str()
                    && !c.is_empty()
                {
                    return Some(Ok(ContentBlock::Text {
                        text: c.to_string(),
                    }));
                }

                if let Some(r) = delta["reasoning_content"].as_str()
                    && !r.is_empty()
                {
                    return Some(Ok(ContentBlock::Thinking {
                        text: r.to_string(),
                    }));
                }

                if let Some(tcs) = delta["tool_calls"].as_array() {
                    for tc in tcs {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        let entry = self
                            .tool_acc
                            .entry(idx)
                            .or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(id) = tc["id"].as_str() {
                            entry.0 = id.to_string();
                        }
                        if let Some(name) = tc["function"]["name"].as_str() {
                            entry.1 = name.to_string();
                        }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.2.push_str(args);
                        }
                    }
                }

                if let Some(fr) = choice["finish_reason"].as_str()
                    && (fr == "tool_calls" || fr == "stop" || fr == "length")
                {
                    self.finalize_tools();
                }
            }
        }
        None
    }

    fn finalize_tools(&mut self) {
        if self.tool_acc.is_empty() {
            return;
        }
        let mut indices: Vec<usize> = self.tool_acc.keys().copied().collect();
        indices.sort();
        for idx in indices {
            let (id, name, args) = self.tool_acc.remove(&idx).unwrap();
            let input: serde_json::Value = serde_json::from_str(&args).unwrap_or(json!({}));
            self.pending_blocks
                .push(ContentBlock::ToolUse { id, name, input });
        }
    }
}

// ── Request conversion ────────────────────────────────────────────

fn build_messages(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();

    for msg in messages {
        match msg.role {
            LlmRole::System => {
                let text = extract_text(&msg.content);
                out.push(json!({"role": "system", "content": text}));
            }
            LlmRole::User => {
                let text = extract_text(&msg.content);
                out.push(json!({"role": "user", "content": text}));
            }
            LlmRole::Assistant => {
                let has_tool_use = msg
                    .content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
                if has_tool_use {
                    let text = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    let tool_calls: Vec<serde_json::Value> = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolUse { id, name, input } => Some(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()),
                                }
                            })),
                            _ => None,
                        })
                        .collect();

                    let mut m = json!({"role": "assistant", "tool_calls": tool_calls});
                    if !text.is_empty() {
                        m["content"] = json!(text);
                    }
                    out.push(m);
                } else {
                    let text = extract_text(&msg.content);
                    out.push(json!({"role": "assistant", "content": text}));
                }
            }
            LlmRole::Tool => {
                for block in &msg.content {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } = block
                    {
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content,
                        }));
                    }
                }
            }
        }
    }

    out
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn build_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
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
        .collect()
}

// ── Response conversion ───────────────────────────────────────────

fn parse_response(
    resp: &serde_json::Value,
    latency: Duration,
) -> Result<CompletionResponse, CasperError> {
    let choice = resp["choices"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| CasperError::BadGateway("OpenAI response has no choices".into()))?;

    let message = &choice["message"];
    let mut content = Vec::new();
    let mut reasoning = Vec::new();

    if let Some(text) = message["content"].as_str()
        && !text.is_empty()
    {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    if let Some(text) = message["reasoning_content"].as_str()
        && !text.is_empty()
    {
        reasoning.push(ContentBlock::Thinking {
            text: text.to_string(),
        });
    }

    if let Some(tool_calls) = message["tool_calls"].as_array() {
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

    let stop_reason = match choice["finish_reason"].as_str() {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        _ => StopReason::EndTurn,
    };

    let usage_obj = &resp["usage"];
    let input_tokens = usage_obj["prompt_tokens"].as_u64().unwrap_or(0) as u32;
    let output_tokens = usage_obj["completion_tokens"].as_u64().unwrap_or(0) as u32;

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

fn map_openai_error(status: u16, body: &str) -> CasperError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| body.chars().take(500).collect());

    match status {
        401 => CasperError::BadGateway(format!("OpenAI auth error: {message}")),
        429 => CasperError::RateLimited,
        500..=599 => CasperError::BadGateway(format!("OpenAI server error ({status}): {message}")),
        _ => CasperError::BadGateway(format!("OpenAI error ({status}): {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_system_message() {
        let messages = vec![
            LlmMessage {
                role: LlmRole::System,
                content: vec![ContentBlock::Text {
                    text: "You are helpful".to_string(),
                }],
            },
            LlmMessage {
                role: LlmRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hi".to_string(),
                }],
            },
        ];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0]["role"], "system");
        assert_eq!(built[1]["role"], "user");
    }

    #[test]
    fn build_assistant_with_tool_calls() {
        let messages = vec![LlmMessage {
            role: LlmRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me check.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    input: json!({"q": "test"}),
                },
            ],
        }];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["role"], "assistant");
        assert_eq!(built[0]["content"], "Let me check.");
        assert_eq!(built[0]["tool_calls"][0]["function"]["name"], "search");
    }

    #[test]
    fn build_tool_result() {
        let messages = vec![LlmMessage {
            role: LlmRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "result data".to_string(),
                is_error: false,
            }],
        }];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["role"], "tool");
        assert_eq!(built[0]["tool_call_id"], "call_1");
        assert_eq!(built[0]["content"], "result data");
    }

    #[test]
    fn parse_openai_response() {
        let resp = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 8,
                "total_tokens": 28
            }
        });

        let result = parse_response(&resp, Duration::from_millis(100)).unwrap();
        assert_eq!(result.content.len(), 1);
        assert!(
            matches!(&result.content[0], ContentBlock::Text { text } if text == "Hello! How can I help?")
        );
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, 20);
        assert_eq!(result.usage.output_tokens, 8);
        assert_eq!(result.model, "gpt-4o");
    }

    #[test]
    fn parse_openai_tool_call() {
        let resp = json!({
            "id": "chatcmpl-def456",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"Zurich\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 30, "completion_tokens": 15}
        });

        let result = parse_response(&resp, Duration::from_millis(50)).unwrap();
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert_eq!(result.content.len(), 1);
        assert!(matches!(
            &result.content[0],
            ContentBlock::ToolUse { name, .. } if name == "get_weather"
        ));
    }

    #[test]
    fn parse_empty_choices_fails() {
        let resp = json!({
            "id": "chatcmpl-err",
            "choices": [],
            "usage": {}
        });
        assert!(parse_response(&resp, Duration::ZERO).is_err());
    }

    #[test]
    fn error_mapping() {
        let err = map_openai_error(429, r#"{"error":{"message":"rate limited"}}"#);
        assert!(matches!(err, CasperError::RateLimited));

        let err = map_openai_error(500, r#"{"error":{"message":"server error"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));
    }
}
