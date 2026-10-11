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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<Cow<'a, str>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Conversation {
    inner: rpds::VectorSync<ConversationItem>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            inner: rpds::VectorSync::new_sync().push_back(ConversationItem::System),
        }
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
                    role: "system".into(),
                    content: SYSTEM_PROMPT.load_full().to_string().into(),
                    reasoning_content: None,
                },
                ConversationItem::User { content } => StandardMessage {
                    role: "user".into(),
                    content: content.into(),
                    reasoning_content: None,
                },
                ConversationItem::Assistant { reasoning, content } => StandardMessage {
                    role: "assistant".into(),
                    content: content.as_str().into(),
                    reasoning_content: reasoning.as_ref().map(|s| s.as_str().into()),
                },
            })
            .collect()
    }
}
