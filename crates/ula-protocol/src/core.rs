use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventFromCore<'a> {
    Streaming(#[serde(borrow)] Message<'a>),
    Content(#[serde(borrow)] Message<'a>),
    /// Reserved for session resume / context management: carries a whole
    /// conversation snapshot instead of one turn. Not consumed by compose yet.
    FullConversation(#[serde(borrow)] Message<'a>),
    Error {
        #[serde(borrow)]
        message: Cow<'a, str>,
    },
}

impl<'a> EventFromCore<'a> {
    pub fn into_owned(self) -> EventFromCore<'static> {
        match self {
            EventFromCore::Streaming(msg) => EventFromCore::Streaming(msg.into_owned()),
            EventFromCore::Content(msg) => EventFromCore::Content(msg.into_owned()),
            EventFromCore::FullConversation(msg) => {
                EventFromCore::FullConversation(msg.into_owned())
            }
            EventFromCore::Error { message } => EventFromCore::Error {
                message: message.into_owned().into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message<'a> {
    pub reasoning: Option<Cow<'a, str>>,
    pub content: Cow<'a, str>,
}

impl<'a> Message<'a> {
    pub fn into_owned(self) -> Message<'static> {
        Message {
            reasoning: self.reasoning.map(|r| r.into_owned().into()),
            content: self.content.into_owned().into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserMessage {
    Text { text: String },
    SystemPrompt { prompt: String },
    Interrupt,
}
