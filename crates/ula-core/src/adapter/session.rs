use std::{
    borrow::Cow,
    collections::VecDeque,
    io::{self, Write, stdout},
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use arc_swap::{ArcSwap, ArcSwapOption};
use async_notify::Notify;
use backon::{BackoffBuilder, ExponentialBuilder, Retryable};
use log::warn;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use smol::stream::StreamExt;
use tap::{Pipe, Tap, TapOptional};
use ula_protocol::core::{EventFromCore, Message};

use crate::{
    adapter::{
        APIEventStream, ModelResponse, Provider,
        conversation::{Conversation, ConversationItem},
        errors::APIError,
        get_provider,
        utils::SmolSleeper,
    },
    config::CONFIG,
};

#[derive(Default)]
struct SessionRuntimeState {
    provider_instance: ArcSwapOption<Provider>,

    stream: Mutex<Option<APIEventStream>>,

    notify: Notify,
    interrupt: Arc<AtomicBool>,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    model: String,
    provider: String,
    conversation: ArcSwap<Conversation>,
    message_queue: Mutex<VecDeque<String>>,

    #[serde(skip)]
    state: SessionRuntimeState,
}

impl Session {
    pub async fn create(
        model_name_with_provider: String,
        interrupt: Arc<AtomicBool>,
    ) -> Result<Self, APIError> {
        let Some((provider, model)) = model_name_with_provider.split_once(':') else {
            return Err(APIError::Fatal {
                message: format!("No provider name in model name {model_name_with_provider}"),
            });
        };
        Ok(Self {
            model: model.to_owned(),
            provider: provider.to_owned(),
            conversation: Default::default(),
            message_queue: Default::default(),

            state: SessionRuntimeState {
                interrupt,
                ..Default::default()
            },
        })
    }

    pub fn save(&self) -> Result<(), io::Error> {
        warn!("Save todo");
        Ok(())
    }

    pub fn queue_message(&self, message: String) {
        self.state.notify.notify_n(NonZero::new(1).unwrap());
        self.message_queue.lock().push_back(message);
    }

    pub async fn poll(&self) -> Result<(), APIError> {
        assert_ne!(
            Arc::strong_count(&self.state.interrupt),
            1,
            "`interrupt` has only 1 reference"
        );

        if self.state.provider_instance.load().is_none()
            && let Err(error) = self.refresh_provider().await
        {
            return self.abort(error).await;
        }

        if self.should_wait() {
            self.state.notify.notified().await;
            return Ok(());
        }

        if let Err(error) = (|| self.ensure_stream())
            .retry(ExponentialBuilder::default().build())
            .when(|err| err.should_retry())
            .sleep(SmolSleeper)
            .await
        {
            return self.abort(error).await;
        }

        #[derive(Debug, Default)]
        struct Accum {
            reasoning: Option<String>,
            content: String,
        }

        let assistant_response = match {
            let mut guard = self.state.stream.lock();
            guard.take().unwrap()
        }
        .try_fold(Accum::default(), |prev: Accum, event| match event {
            ModelResponse::Delta { reasoning, content } => {
                write_event(&EventFromCore::Streaming(Message {
                    reasoning: reasoning.as_ref().map(Into::into),
                    content: Cow::Borrowed(&content),
                }));
                Accum {
                    reasoning: prev
                        .reasoning
                        .tap_some_mut(|prev_reasoning| {
                            prev_reasoning.push_str(reasoning.as_deref().unwrap_or(""))
                        })
                        .or(reasoning),
                    content: prev.content.tap_mut(|x| x.push_str(&content)),
                }
                .pipe(Ok)
            }
            ModelResponse::Usage => {
                warn!("TODO: usage");
                Ok(prev)
            }
            ModelResponse::End => {
                warn!("TODO: end");
                Ok(prev)
            }
        })
        .await
        {
            Ok(response) => response,
            Err(error) => return self.abort(error).await,
        };

        write_event(&EventFromCore::Content(Message {
            reasoning: assistant_response.reasoning.as_ref().map(Into::into),
            content: Cow::Borrowed(&assistant_response.content),
        }));

        let message = ConversationItem::Assistant {
            reasoning: assistant_response.reasoning,
            content: assistant_response.content,
        };

        self.conversation
            .rcu(|conversation| conversation.push_new(message.clone()));
        self.save()?;

        Ok(())
    }

    #[inline]
    fn should_wait(&self) -> bool {
        self.state
            .interrupt
            .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
            || (self.state.stream.lock().is_none() && self.message_queue.lock().is_empty())
    }

    pub fn interrupt(&self) {
        self.state.interrupt.store(true, Ordering::Release);
        self.state.notify.notify();
    }

    /// Ends the current turn on a request error: drops the provider, reports the
    /// error to the UI and suspends until the user sends the next message.
    async fn abort(&self, error: APIError) -> Result<(), APIError> {
        warn!("poll aborted: {error}");
        self.state.provider_instance.store(None);
        write_event(&EventFromCore::Error { message: error.to_string().into() });
        self.state.notify.notified().await;
        Ok(())
    }

    async fn refresh_provider(&self) -> Result<(), APIError> {
        let provider_config = CONFIG
            .load()
            .providers
            .get(&self.provider)
            .cloned()
            .ok_or_else(|| APIError::Retryable {
                message: format!("No provider named `{}`", self.provider),
            })?;
        let provider = get_provider(provider_config).await?;

        self.state.provider_instance.store(Some(Arc::new(provider)));
        Ok(())
    }

    async fn ensure_stream(&self) -> Result<(), APIError> {
        if self.state.stream.lock().is_none()
            && let Some(message) = {
                let mut guard = self.message_queue.lock();
                guard.pop_front()
            }
        {
            let stream = self
                .state
                .provider_instance
                .load()
                .as_ref()
                .unwrap()
                .ask(&message, &self.model, &self.conversation.load())
                .await
                .inspect_err(|_| self.message_queue.lock().push_back(message.clone()))?
                // This clone is necessary because of Rust's borrow checker does not understand it
                .boxed();

            self.state.stream.lock().replace(stream);

            self.conversation.rcu(|conversation| {
                conversation.push_new(ConversationItem::User {
                    content: message.clone(),
                })
            });
            self.save()?;
        }
        Ok(())
    }
}

#[inline]
fn write_event(event: &EventFromCore) {
    writeln!(
        stdout(),
        "{}",
        serde_json::to_string(event).expect("SAFETY: serialize will never fail")
    )
    .unwrap_or_default();
}
