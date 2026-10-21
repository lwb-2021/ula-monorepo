use std::{
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
use ula_protocol::core::EventFromCore;
use ula_protocol::universal::{MessageEvent, MessageEventType};

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

    last_usage: Mutex<Option<u32>>,
    compacting: AtomicBool,
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
            return APIError::Fatal {
                message: format!("No provider name in model name {model_name_with_provider}"),
            }
            .raise();
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
        self.message_queue.lock().push_back(message);
        self.state.notify.notify_n(NonZero::new(1).unwrap());
    }

    pub fn compact(&self, summary: String) {
        self.conversation
            .rcu(|conversation| conversation.compact(summary.clone()));
        self.state.last_usage.lock().take();
        self.state.compacting.store(false, Ordering::Release);
        self.state.notify.notify();
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

        if self.state.compacting.load(Ordering::Acquire) {
            self.state.notify.notified().await;
            return Ok(());
        }

        if self.should_compact() {
            self.state.compacting.store(true, Ordering::Release);
            write_event(&EventFromCore::CompactRequest {
                full_conversation: self.conversation.load().to_full_conversation_string(),
            });
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
                write_event(&EventFromCore::Message {
                    message: MessageEvent {
                        reasoning: reasoning.clone(),
                        content: content.clone(),
                        message_type: MessageEventType::Streaming,
                    },
                });

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
            ModelResponse::Usage { total, .. } => {
                self.state.last_usage.lock().replace(total);
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

        write_event(&EventFromCore::Message {
            message: MessageEvent {
                reasoning: assistant_response.reasoning.clone(),
                content: assistant_response.content.clone(),
                message_type: MessageEventType::Content,
            },
        });

        let message =
            ConversationItem::assistant(assistant_response.reasoning, assistant_response.content);

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

    fn should_compact(&self) -> bool {
        let config = CONFIG.load();
        let Some(window) = config.context_window else {
            return false;
        };
        self.state
            .last_usage
            .lock()
            .is_some_and(|usage| usage as f32 >= config.context_limit * window as f32)
    }

    pub fn interrupt(&self) {
        self.state.interrupt.store(true, Ordering::Release);
    }

    async fn abort(&self, error: APIError) -> Result<(), APIError> {
        warn!("poll aborted: {error}");
        self.state.provider_instance.store(None);
        write_event(&EventFromCore::Error {
            error_msg: error.to_string(),
        });
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

            self.conversation
                .rcu(|conversation| conversation.push_new(ConversationItem::user(message.clone())));
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
