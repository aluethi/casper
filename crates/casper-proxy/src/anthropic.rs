use casper_base::CasperError;
use serde_json::json;

use crate::types::{LlmRequest, LlmResponse};

/// Send a request to the Anthropic Messages API and parse the response.
pub async fn call(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
) -> Result<LlmResponse, CasperError> {
    // Build the Anthropic request body.
    // Anthropic expects system message extracted from messages, and uses a
    // different format for the messages array.
    let (system_content, messages) = extract_system_and_messages(&request.messages);

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(4096),
    });

    if let Some(system) = system_content {
        body["system"] = system;
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }

    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
    }

    // Merge extra params (e.g. top_k, top_p, metadata)
    if let serde_json::Value::Object(ref extra) = request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in extra {
            // Don't overwrite fields we already set
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    // Ensure URL ends correctly
    let url = format!(
        "{}/v1/messages",
        base_url.trim_end_matches('/')
    );

    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Anthropic request failed: {e}")))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Failed to read Anthropic response: {e}")))?;

    if !status.is_success() {
        return Err(map_anthropic_error(status.as_u16(), &response_text));
    }

    let resp: serde_json::Value = serde_json::from_str(&response_text)
        .map_err(|e| CasperError::BadGateway(format!("Invalid Anthropic JSON: {e}")))?;

    parse_response(&resp)
}

/// Extract system message from the messages list. Anthropic expects system
/// as a top-level field, not as a message with role "system".
fn extract_system_and_messages(
    messages: &[crate::types::Message],
) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system = None;
    let mut out = Vec::new();

    for msg in messages {
        if msg.role == "system" {
            // Anthropic system can be a string or array of content blocks
            system = Some(msg.content.clone());
        } else {
            out.push(json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }
    }

    (system, out)
}

/// Parse an Anthropic Messages API response into our LlmResponse.
fn parse_response(resp: &serde_json::Value) -> Result<LlmResponse, CasperError> {
    // Extract text content from content blocks
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    if let Some(content_arr) = resp["content"].as_array() {
        for block in content_arr {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        text_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    tool_calls.push(block.clone());
                }
                _ => {}
            }
        }
    }

    let content = text_parts.join("");

    // Extract usage
    let usage = &resp["usage"];
    let input_tokens = usage["input_tokens"].as_i64().unwrap_or(0) as i32;
    let output_tokens = usage["output_tokens"].as_i64().unwrap_or(0) as i32;
    let cache_read_tokens = usage["cache_read_input_tokens"]
        .as_i64()
        .map(|v| v as i32);
    let cache_write_tokens = usage["cache_creation_input_tokens"]
        .as_i64()
        .map(|v| v as i32);

    let finish_reason = resp["stop_reason"].as_str().map(|s| {
        // Map Anthropic stop reasons to OpenAI-compatible ones
        match s {
            "end_turn" => "stop".to_string(),
            "max_tokens" => "length".to_string(),
            "tool_use" => "tool_calls".to_string(),
            other => other.to_string(),
        }
    });

    let model = resp["model"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    Ok(LlmResponse {
        content,
        role: "assistant".to_string(),
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        finish_reason,
    })
}

/// Map Anthropic HTTP status codes to CasperError variants.
fn map_anthropic_error(status: u16, body: &str) -> CasperError {
    // Try to extract error message from JSON
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| body.chars().take(500).collect());

    match status {
        401 => CasperError::BadGateway(format!("Anthropic auth error: {message}")),
        429 => CasperError::RateLimited,
        500..=599 => CasperError::BadGateway(format!("Anthropic server error ({status}): {message}")),
        _ => CasperError::BadGateway(format!("Anthropic error ({status}): {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_system_message() {
        let messages = vec![
            crate::types::Message {
                role: "system".to_string(),
                content: json!("You are a helpful assistant"),
            },
            crate::types::Message {
                role: "user".to_string(),
                content: json!("Hello"),
            },
        ];

        let (system, msgs) = extract_system_and_messages(&messages);
        assert_eq!(system, Some(json!("You are a helpful assistant")));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn extract_no_system() {
        let messages = vec![crate::types::Message {
            role: "user".to_string(),
            content: json!("Hello"),
        }];

        let (system, msgs) = extract_system_and_messages(&messages);
        assert!(system.is_none());
        assert_eq!(msgs.len(), 1);
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
                "cache_read_input_tokens": 5,
                "cache_creation_input_tokens": 20
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "Hello! How can I help?");
        assert_eq!(llm.role, "assistant");
        assert_eq!(llm.input_tokens, 25);
        assert_eq!(llm.output_tokens, 10);
        assert_eq!(llm.cache_read_tokens, Some(5));
        assert_eq!(llm.cache_write_tokens, Some(20));
        assert_eq!(llm.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn parse_anthropic_tool_use_response() {
        let resp = json!({
            "id": "msg_456",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Let me search for that."},
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "search",
                    "input": {"query": "rust programming"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 30
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "Let me search for that.");
        assert!(llm.tool_calls.is_some());
        assert_eq!(llm.tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(llm.finish_reason, Some("tool_calls".to_string()));
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
}
