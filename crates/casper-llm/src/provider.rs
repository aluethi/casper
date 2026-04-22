use std::pin::Pin;

use casper_base::CasperError;
use futures::Stream;

use crate::types::{CompletionRequest, CompletionResponse, ContentBlock};

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;

    async fn complete(&self, request: CompletionRequest)
    -> Result<CompletionResponse, CasperError>;

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>;
}
