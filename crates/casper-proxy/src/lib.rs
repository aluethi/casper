/// LLM provider dispatch: Anthropic and OpenAI-compatible adapters.
pub mod anthropic;
pub mod dispatch;
pub mod openai;
pub mod types;

pub use dispatch::{dispatch, dispatch_with_retry};
pub use types::{LlmRequest, LlmResponse, Message};
