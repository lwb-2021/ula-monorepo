mod openai_chat_completion;
#[cfg(test)]
mod tests;
mod utils;

use std::pin::Pin;

use async_trait::async_trait;
use once_cell::sync::OnceCell;
use smol::stream::Stream;

use crate::{
    adapter::{
        conversation::Conversation, errors::APIError,
        provider::openai_chat_completion::OpenAICompatibleChatCompletionProviderBehavior,
    },
    config::ProviderConfig,
};

pub type APIEventStream = Pin<Box<dyn Stream<Item = Result<ModelResponse, APIError>> + Send>>;

#[derive(Debug, Clone)]
pub enum ModelResponse {
    Delta {
        reasoning: Option<String>,
        content: String,
    },
    Usage,
    End,
}

#[async_trait]
pub trait ProviderBehavior: std::fmt::Debug + Send + Sync {
    async fn ask(
        &self,
        input: &str,
        model: &str,
        info: &ProviderInfo,
        conversation: &Conversation,
    ) -> Result<APIEventStream, APIError>;
}

#[derive(Debug)]
pub struct ProviderInfo {
    base_url: String, // No suffix
    api_key_command: String,
    session: OnceCell<isahc::HttpClient>,
}

#[derive(Debug)]
pub struct Provider {
    pub(super) inner: Box<dyn ProviderBehavior>,
    pub(super) info: ProviderInfo,
}

impl Provider {
    pub async fn ask(
        &self,
        input: &str,
        model: &str,
        conversation: &Conversation,
    ) -> Result<APIEventStream, APIError> {
        self.inner.ask(input, model, &self.info, conversation).await
    }
}

pub async fn get_provider(config: ProviderConfig) -> Result<Provider, APIError> {
    let base_url = config
        .base_url
        .strip_suffix('/')
        .unwrap_or(config.base_url.as_str())
        .to_string();

    match config.endpoint.as_str() {
        "chat_completions" => Ok(Provider {
            inner: Box::new(OpenAICompatibleChatCompletionProviderBehavior),
            info: ProviderInfo {
                base_url,
                api_key_command: config.api_key_command.clone(),
                session: OnceCell::new(),
            },
        }),
        endpoint => Err(APIError::UnsupportedEndpoint {
            name: endpoint.to_string(),
        }),
    }
}
