use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::SYSTEM_PROMPT;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConversationItem {
    System,

    User {
        content: String,
    },

    Assistant {
        reasoning: Option<String>,
        content: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StandardMessage<'a> {
    pub role: Cow<'a, str>,
    pub content: Cow<'a, str>,
    pub reasoning_content: Option<Cow<'a, str>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Conversation {
    inner: rpds::VectorSync<ConversationItem>,
}

impl Default for Conversation {
    fn default() -> Self {
        let result = Self {
            inner: rpds::VectorSync::new_sync().push_back(ConversationItem::System),
        };
        result
    }
}

impl Conversation {
    pub fn push_new(&self, message: ConversationItem) -> Self {
        Self {
            inner: self.inner.push_back(message),
        }
    }
    pub fn build_standard_messages(&self) -> Vec<StandardMessage<'_>> {
        self.inner
            .iter()
            .map(|item| match item {
                ConversationItem::System => StandardMessage {
                    role: Cow::Borrowed("system"),
                    content: Cow::Owned(SYSTEM_PROMPT.load_full().to_string()),
                    reasoning_content: None,
                },
                ConversationItem::User { content } => StandardMessage {
                    role: Cow::Borrowed("user"),
                    content: Cow::Borrowed(content),
                    reasoning_content: None,
                },
                ConversationItem::Assistant { reasoning, content } => StandardMessage {
                    role: Cow::Borrowed("assistant"),
                    content: Cow::Borrowed(content.as_str()),
                    reasoning_content: reasoning.as_ref().map(|s| s.as_str().into()),
                },
            })
            .collect()
    }
}
