use std::{
    collections::VecDeque,
    io::{Write, stdout},
    sync::Arc,
};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use smol::{Timer, channel::Receiver, stream::StreamExt};
use tap::{Tap, TapOptional};
use ula_protocol::{
    core::{EventFromCore, EventToCore},
    universal::{MessageEvent, MessageEventType, UserInteraction},
};

use crate::{
    SYSTEM_PROMPT,
    adapter::{
        APIEventStream, ModelResponse,
        conversation::{Conversation, ConversationItem},
        errors::APIError,
        get_provider,
        session::{
            context::SessionContext,
            state::{Partial, SessionState},
        },
    },
    config::CONFIG,
};

#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    model: String,
    provider: String,
    conversation: Conversation,
    message_queue: VecDeque<String>,

    #[serde(skip)]
    context: SessionContext,
    #[serde(skip)]
    state: SessionState,
}

impl Session {
    pub async fn create(model_name_with_provider: String) -> Result<Self, APIError> {
        let Some((provider, model)) = model_name_with_provider.split_once(':') else {
            return APIError::Fatal {
                message: format!("No provider name in model name {model_name_with_provider}"),
            }
            .raise();
        };
        Ok(Self {
            model: model.to_owned(),
            provider: provider.to_owned(),

            ..Default::default()
        })
    }

    pub async fn serve(&mut self, rx: &Receiver<EventToCore>) -> eyre::Result {
        loop {
            while let Ok(event) = rx.try_recv() {
                self.context.event_queue.push_back(event);
            }

            while let Some(event) = self.context.event_queue.pop_front() {
                if let Some(state) = self.on_event(event).await {
                    self.state = state;
                    break;
                }
            }

            self.machine(&rx).await?;
        }
    }

    async fn machine(&mut self, rx: &Receiver<EventToCore>) -> eyre::Result {
        self.state = match std::mem::take(&mut self.state) {
            SessionState::Idle => 'b: {
                if self.context.provider_instance.is_none()
                    && let Err(err) = self.refresh_provider().await
                {
                    break 'b SessionState::Error { error: err };
                }
                match self.message_queue.pop_front() {
                    Some(message) => self.start_streaming(message).await,
                    None => SessionState::Halt,
                }
            }
            SessionState::Halt => match rx.recv().await {
                Ok(event) => self.on_event(event).await.unwrap_or(SessionState::Idle),
                Err(_err) => {
                    info!("Exiting");
                    return Ok(());
                }
            },
            SessionState::Error { error } => {
                write_event(&EventFromCore::Error {
                    error_msg: error.to_string(),
                });

                SessionState::Halt
            }

            SessionState::Streaming {
                stream,
                partial,
                usage,
            } => self.do_streaming(stream, partial, usage).await,

            SessionState::WaitCompact => match rx.recv().await {
                Ok(EventToCore::Compact { summary }) => SessionState::Compact { summary },
                Ok(event) => {
                    self.context.event_queue.push_back(event);
                    SessionState::WaitCompact
                }
                Err(_err) => {
                    info!("Exiting");
                    return Ok(());
                }
            },
            SessionState::Compact { summary } => {
                self.conversation = self.conversation.compact(summary);
                SessionState::Idle
            }
        };
        Ok(())
    }

    pub fn save(&self) -> eyre::Result<()> {
        warn!("TODO: persist the session once the window table exists");
        Ok(())
    }

    async fn on_event(&mut self, event: EventToCore) -> Option<SessionState> {
        match event {
            EventToCore::Compact { summary } => Some(SessionState::Compact { summary }), // TODO:
            // queue compact when streaming
            EventToCore::UserInteraction {
                interaction: UserInteraction::Text { text },
            } => {
                self.message_queue.push_back(text);
                None
            }

            EventToCore::UserInteraction {
                interaction: UserInteraction::Interrupt,
            } => {
                if let SessionState::Streaming { partial, .. } = &self.state {
                    write_event(&EventFromCore::Message {
                        message: MessageEvent {
                            reasoning: partial.reasoning.clone(),
                            content: partial.content.clone(),
                            message_type: MessageEventType::Content,
                        },
                    });
                    self.conversation = self.conversation.push_new(ConversationItem::assistant(
                        partial.reasoning.clone(),
                        partial.content.clone(),
                    ));
                };
                Some(SessionState::Halt)
            }

            EventToCore::UserInteraction {
                interaction: UserInteraction::ExtraSystemPrompt { prompt },
            } => {
                extend_system_prompt(&prompt);
                None
            }
        }
    }

    async fn start_streaming(&mut self, message: String) -> SessionState {
        loop {
            match self
                .context
                .provider_instance
                .as_ref()
                .unwrap()
                .ask(&message, &self.model, &self.conversation)
                .await
            {
                Ok(stream) => {
                    self.conversation = self.conversation.push_new(ConversationItem::user(message));

                    break SessionState::Streaming {
                        stream,
                        partial: Default::default(),
                        usage: None,
                    };
                }

                Err(err) if !err.should_retry() => break SessionState::Error { error: err },
                Err(err) => {
                    let Some(duration) = self.context.retry.retry() else {
                        self.message_queue.push_front(message);
                        self.context.retry.reset_attempts();

                        break SessionState::Error { error: err };
                    };
                    Timer::after(duration).await;
                }
            }
        }
    }

    async fn do_streaming(
        &mut self,
        mut stream: APIEventStream,
        partial: Partial,
        usage: Option<u32>,
    ) -> SessionState {
        match stream.next().await {
            Some(Ok(item)) => match item {
                ModelResponse::Delta { reasoning, content } => {
                    write_event(&EventFromCore::Message {
                        message: MessageEvent {
                            reasoning: reasoning.clone(),
                            content: content.clone(),
                            message_type: MessageEventType::Streaming,
                        },
                    });

                    SessionState::Streaming {
                        stream,
                        partial: Partial {
                            reasoning: partial
                                .reasoning
                                .tap_some_mut(|prev_reasoning| {
                                    prev_reasoning.push_str(reasoning.as_deref().unwrap_or(""))
                                })
                                .or(reasoning),
                            content: partial.content.tap_mut(|x| x.push_str(&content)),
                        },
                        usage: None,
                    }
                }
                ModelResponse::Usage { total, .. } => {
                    warn!("TODO: deal with usage");
                    SessionState::Streaming {
                        stream,
                        partial,
                        usage: Some(total),
                    }
                }
                ModelResponse::End => {
                    write_event(&EventFromCore::Message {
                        message: MessageEvent {
                            reasoning: partial.reasoning.clone(),
                            content: partial.content.clone(),
                            message_type: MessageEventType::Content,
                        },
                    });

                    self.conversation = self.conversation.push_new(ConversationItem::assistant(
                        partial.reasoning,
                        partial.content,
                    ));

                    let threhold = self.get_compact_threhold().await;
                    if usage.is_some_and(|usage| usage > threhold) {
                        write_event(&EventFromCore::CompactRequest {
                            full_conversation: self.conversation.to_full_conversation_string(),
                        });
                        return SessionState::WaitCompact;
                    }
                    SessionState::Idle
                }
            },

            Some(Err(err)) if !err.should_retry() => SessionState::Error { error: err },
            Some(Err(err)) => {
                let Some(duration) = self.context.retry.retry() else {
                    self.context.retry.reset_attempts();
                    return SessionState::Error { error: err };
                };
                Timer::after(duration).await;

                let Some((ConversationItem::User { content }, conversation)) =
                    self.conversation.pop()
                else {
                    panic!(
                        "Illegal state: message_type: expected `User`, got `Asistant` or `System`"
                    );
                };
                self.conversation = conversation;
                self.start_streaming(content).await
            }

            None => SessionState::Idle,
        }
    }

    async fn get_compact_threhold(&self) -> u32 {
        warn!("TODO: remove hardcoded context length");
        838860
    }

    async fn refresh_provider(&mut self) -> Result<(), APIError> {
        let provider_config = CONFIG
            .load()
            .providers
            .get(&self.provider)
            .cloned()
            .ok_or_else(|| APIError::Fatal {
                message: format!("No provider named `{}`", self.provider),
            })?;
        let provider = get_provider(provider_config).await?;

        self.context.provider_instance = Some(provider);
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

fn extend_system_prompt(extra: &str) {
    SYSTEM_PROMPT.rcu(|current| Arc::new(format!("{}\n\n{extra}", **current)));
    info!("system prompt extended by {} bytes", extra.len());
}
