use async_trait::async_trait;
use asynchronous_codec::FramedRead;
use isahc::Request;
use serde::{Deserialize, Serialize};
use smol::stream::{StreamExt, try_unfold};
use tap::Pipe;

use crate::adapter::{
    APIError,
    conversation::{Conversation, StandardMessage},
    provider::{
        APIEventStream, ModelResponse, ProviderBehavior, ProviderInfo,
        utils::{HttpExt, get_standard_session_creator},
    },
    sse_stream::{SSEDecoder, SSEItem},
};

#[derive(Debug, Serialize)]
struct RequestPayload<'a> {
    model: &'a str,
    messages: Vec<StandardMessage<'a>>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct Response {
    id: String,
    choices: Vec<Choices>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choices {
    finish_reason: Option<String>,
    index: u8,
    delta: Delta,
}

#[derive(Debug, Deserialize)]
struct Delta {
    role: String,
    reasoning: Option<String>,
    reasoning_content: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    prompt_tokens_details: PromptTokensDetails,
}

#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    cached_tokens: u32,
}

#[derive(Debug)]
pub struct OpenAICompatibleChatCompletionProviderBehavior;

#[async_trait]
impl ProviderBehavior for OpenAICompatibleChatCompletionProviderBehavior {
    async fn ask(
        &self,
        input: &str,
        model: &str,
        info: &ProviderInfo,
        conversation: &Conversation,
    ) -> Result<APIEventStream, APIError> {
        let session = info
            .session
            .get_or_try_init(get_standard_session_creator(&info.api_key_command).await?)?;

        let mut messages = conversation.build_standard_messages();
        messages.push(StandardMessage {
            role: "user".into(),
            content: input.to_owned().into(),
            reasoning_content: None,
        });

        let req_payload = serde_json::to_string(&RequestPayload {
            model,
            messages,
            stream: true,
        })?;

        let request = Request::builder()
            .uri(format!("{}/chat/completions", info.base_url))
            .method("POST")
            .body(req_payload)
            .expect("SAFETY: No error should be in http request");

        let response = session.send_async(request).await?.raise_for_status()?;

        let framed = FramedRead::new(response.into_body(), SSEDecoder);
        Ok(try_unfold(framed, |mut stream| async move {
            match stream.next().await {
                Some(Ok(SSEItem::Data(data))) => {
                    let resp: Response = serde_json::from_value(data)?;
                    if resp.choices.is_empty() {
                        let Some(usage) = resp.usage else {
                            return APIError::InvalidAPIFormat {
                                message: format!("Expected usage information, found null"),
                            }
                            .raise();
                        };
                        return (
                            ModelResponse::Usage {
                                total: usage.total_tokens,
                                cached: usage.prompt_tokens_details.cached_tokens,
                            },
                            stream,
                        )
                            .pipe(Some)
                            .pipe(Ok);
                    }

                    let delta = &resp.choices[0].delta;
                    (
                        ModelResponse::Delta {
                            reasoning: delta
                                .reasoning
                                .as_ref()
                                .or(delta.reasoning_content.as_ref())
                                .cloned(),
                            content: delta.content.clone().unwrap_or_default(),
                        },
                        stream,
                    )
                        .pipe(Some)
                        .pipe(Ok)
                }
                Some(Ok(SSEItem::Done)) => Ok(Some((ModelResponse::End, stream))),
                Some(Err(err)) => Err(err)?,
                None => Ok(None),
            }
        })
        .boxed())
    }
}
