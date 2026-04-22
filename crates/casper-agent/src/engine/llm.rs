//! Mock LLM provider for testing.

#[cfg(test)]
use std::pin::Pin;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use casper_base::CasperError;
#[cfg(test)]
use casper_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmProvider, StopReason, TokenUsage,
};
#[cfg(test)]
use futures::Stream;

#[cfg(test)]
pub struct MockLlmProvider {
    responses: std::sync::Mutex<Vec<CompletionResponse>>,
}

#[cfg(test)]
impl MockLlmProvider {
    pub fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
        }
    }

    pub fn simple(text: &str) -> Self {
        Self::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            reasoning: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
            },
            model: "mock-model".to_string(),
            latency: Duration::from_millis(10),
        }])
    }

    pub fn with_tool_call(
        tool_name: &str,
        tool_input: serde_json::Value,
        final_text: &str,
    ) -> Self {
        Self::new(vec![
            CompletionResponse {
                content: vec![ContentBlock::ToolUse {
                    id: "call_001".to_string(),
                    name: tool_name.to_string(),
                    input: tool_input,
                }],
                reasoning: vec![],
                stop_reason: StopReason::ToolUse,
                usage: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                },
                model: "mock-model".to_string(),
                latency: Duration::from_millis(10),
            },
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: final_text.to_string(),
                }],
                reasoning: vec![],
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage {
                    input_tokens: 150,
                    output_tokens: 60,
                },
                model: "mock-model".to_string(),
                latency: Duration::from_millis(10),
            },
        ])
    }

    fn next_response(&self) -> Result<CompletionResponse, CasperError> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(CasperError::Internal(
                "MockLlmProvider: no more responses".into(),
            ));
        }
        Ok(responses.remove(0))
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, CasperError> {
        self.next_response()
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>
    {
        let response = self.next_response()?;
        let blocks: Vec<Result<ContentBlock, CasperError>> =
            response.content.into_iter().map(Ok).collect();
        Ok(Box::pin(futures::stream::iter(blocks)))
    }
}
