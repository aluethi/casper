use casper_base::CasperError;
use serde_json::json;

use crate::types::{LlmRequest, LlmResponse};

/// Send a request to an OpenAI-compatible Chat Completions API and parse the response.
pub async fn call(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
) -> Result<LlmResponse, CasperError> {
    let messages = build_messages(&request.messages);

    let mut body = json!({
        "model": request.model,
        "messages": messages,
    });

    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }

    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
    }

    // Merge extra params
    if let serde_json::Value::Object(ref extra) = request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in extra {
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    // Try both common URL patterns
    let base = base_url.trim_end_matches('/');
    let url = if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };

    let response = client
        .post(&url)
        .header("authorization", format!("Bearer {api_key}"))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("OpenAI request failed: {e}")))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Failed to read OpenAI response: {e}")))?;

    if !status.is_success() {
        return Err(map_openai_error(status.as_u16(), &response_text));
    }

    let resp: serde_json::Value = serde_json::from_str(&response_text)
        .map_err(|e| CasperError::BadGateway(format!("Invalid OpenAI JSON: {e}")))?;

    parse_response(&resp)
}

/// Build OpenAI-format messages array. OpenAI accepts system messages inline.
fn build_messages(messages: &[crate::types::Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|msg| {
            json!({
                "role": msg.role,
                "content": msg.content,
            })
        })
        .collect()
}

/// Parse an OpenAI Chat Completions response into our LlmResponse.
fn parse_response(resp: &serde_json::Value) -> Result<LlmResponse, CasperError> {
    let choice = resp["choices"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| CasperError::BadGateway("OpenAI response has no choices".into()))?;

    let message = &choice["message"];

    let content = message["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let role = message["role"]
        .as_str()
        .unwrap_or("assistant")
        .to_string();

    // Extract tool calls if present
    let tool_calls = message["tool_calls"]
        .as_array()
        .map(|arr| arr.to_vec())
        .filter(|arr| !arr.is_empty());

    let finish_reason = choice["finish_reason"]
        .as_str()
        .map(|s| s.to_string());

    // Usage
    let usage = &resp["usage"];
    let input_tokens = usage["prompt_tokens"].as_i64().unwrap_or(0) as i32;
    let output_tokens = usage["completion_tokens"].as_i64().unwrap_or(0) as i32;

    // Some OpenAI-compatible APIs include cache token info
    let cache_read_tokens = usage["prompt_tokens_details"]["cached_tokens"]
        .as_i64()
        .map(|v| v as i32);

    let model = resp["model"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    Ok(LlmResponse {
        content,
        role,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens: None,
        tool_calls,
        finish_reason,
    })
}

/// Map OpenAI HTTP status codes to CasperError variants.
fn map_openai_error(status: u16, body: &str) -> CasperError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .map(|s| s.to_string())
        })
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
    use crate::types::Message;

    #[test]
    fn build_messages_preserves_system() {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: json!("You are helpful"),
            },
            Message {
                role: "user".to_string(),
                content: json!("Hi"),
            },
        ];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0]["role"], "system");
        assert_eq!(built[1]["role"], "user");
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

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "Hello! How can I help?");
        assert_eq!(llm.role, "assistant");
        assert_eq!(llm.model, "gpt-4o");
        assert_eq!(llm.input_tokens, 20);
        assert_eq!(llm.output_tokens, 8);
        assert!(llm.cache_read_tokens.is_none());
        assert_eq!(llm.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn parse_openai_tool_call_response() {
        let resp = json!({
            "id": "chatcmpl-def456",
            "object": "chat.completion",
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
            "usage": {
                "prompt_tokens": 30,
                "completion_tokens": 15,
                "total_tokens": 45
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "");
        assert!(llm.tool_calls.is_some());
        assert_eq!(llm.tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(llm.finish_reason, Some("tool_calls".to_string()));
    }

    #[test]
    fn parse_openai_cached_tokens() {
        let resp = json!({
            "id": "chatcmpl-ghi789",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Cached response"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "total_tokens": 110,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.cache_read_tokens, Some(80));
    }

    #[test]
    fn parse_empty_choices_fails() {
        let resp = json!({
            "id": "chatcmpl-err",
            "object": "chat.completion",
            "choices": [],
            "usage": {}
        });

        let result = parse_response(&resp);
        assert!(result.is_err());
    }

    #[test]
    fn error_mapping() {
        let err = map_openai_error(429, r#"{"error":{"message":"rate limited"}}"#);
        assert!(matches!(err, CasperError::RateLimited));

        let err = map_openai_error(500, r#"{"error":{"message":"server error"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));
    }
}
