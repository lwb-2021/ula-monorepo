use async_trait::async_trait;

use crate::adapter::{
    APIError, APIEventStream,
    conversation::Conversation,
    provider::{ProviderBehavior, ProviderInfo},
};

#[derive(Debug)]
pub struct FakeProvider;
#[async_trait]
impl ProviderBehavior for FakeProvider {
    async fn ask(
        &self,
        input: &str,
        model: &str,
        info: &ProviderInfo,
        conversation: &Conversation,
    ) -> Result<APIEventStream, APIError> {
        todo!()
    }
}
