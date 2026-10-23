use std::borrow::Cow;

use eyre::OptionExt;
use serde::{Deserialize, Serialize};
use tap::Tap;

use crate::SYSTEM_PROMPT;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl ConversationItem {
    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
        }
    }

    pub fn assistant(reasoning: Option<String>, content: impl Into<String>) -> Self {
        Self::Assistant {
            reasoning,
            content: content.into(),
        }
    }
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
    last_visible_index: usize,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            inner: rpds::VectorSync::new_sync().push_back(ConversationItem::System),
            last_visible_index: 0,
        }
    }
}

impl Conversation {
    pub fn push_new(&self, message: ConversationItem) -> Self {
        Self {
            inner: self.inner.push_back(message),
            last_visible_index: self.last_visible_index,
        }
    }

    pub fn pop(&self) -> Option<(ConversationItem, Self)> {
        assert!(
            self.inner.len() > self.last_visible_index,
            "cannot pop the archived prefix"
        );

        Some((
            self.inner.last()?.clone(),
            Self {
                inner: self.inner.drop_last()?,
                last_visible_index: self.last_visible_index,
            },
        ))
    }

    pub fn revert(&self, drop_nums: usize) -> eyre::Result<Self> {
        let remaining = self
            .inner
            .len()
            .checked_sub(drop_nums)
            .filter(|len| *len > self.last_visible_index)
            .ok_or_eyre("Unable to revert: dropping too many items")?;

        let mut inner = self.inner.clone();
        for _ in remaining..self.inner.len() {
            inner = inner
                .drop_last()
                .expect("invariant: loop only runs while the vector is non-empty");
        }

        Ok(Self {
            inner,
            last_visible_index: self.last_visible_index,
        })
    }

    pub fn compact(&self, summary: String) -> Self {
        let last_visible_index = self.inner.len();
        Self {
            inner: self
                .inner
                .push_back(ConversationItem::System)
                .push_back(ConversationItem::User { content: summary }),
            last_visible_index,
        }
    }

    pub fn view(&self) -> impl Iterator<Item = &ConversationItem> {
        self.inner.iter().skip(self.last_visible_index)
    }

    /// The system prompt is per-window, so it must never end up in a summary.
    pub fn to_full_conversation_string(&self) -> String {
        self.view().fold(String::new(), |prev, item| {
            prev.tap_mut(|prev| {
                prev.push_str(&match item {
                    ConversationItem::System => String::new(),
                    ConversationItem::User { content } => format!("<USER>\n{content}\n</USER>"),
                    ConversationItem::Assistant {
                        reasoning: _,
                        content,
                    } => {
                        format!("<ASSISTANT>\n{content}\n</ASSISTANT>")
                    }
                })
            })
        })
    }

    pub fn build_standard_messages(&self) -> Vec<StandardMessage<'_>> {
        self.view()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(conversation: &Conversation) -> Vec<ConversationItem> {
        conversation.view().cloned().collect()
    }

    #[test]
    fn compact_hides_the_archived_prefix() {
        let conversation = Conversation::default()
            .push_new(ConversationItem::user("a"))
            .compact("summary".into());

        assert_eq!(
            visible(&conversation),
            [ConversationItem::System, ConversationItem::user("summary")]
        );
    }

    #[test]
    fn push_preserves_the_compaction_window() {
        let conversation = Conversation::default()
            .push_new(ConversationItem::user("a"))
            .compact("summary".into())
            .push_new(ConversationItem::user("b"));

        assert_eq!(
            visible(&conversation),
            [
                ConversationItem::System,
                ConversationItem::user("summary"),
                ConversationItem::user("b"),
            ]
        );
    }

    #[test]
    fn revert_drops_the_tail_without_touching_the_original() {
        let conversation = Conversation::default()
            .push_new(ConversationItem::user("a"))
            .push_new(ConversationItem::user("b"));

        let reverted = conversation.revert(1).unwrap();

        assert_eq!(
            visible(&reverted),
            [ConversationItem::System, ConversationItem::user("a")]
        );
        assert_eq!(
            visible(&conversation),
            [
                ConversationItem::System,
                ConversationItem::user("a"),
                ConversationItem::user("b"),
            ]
        );
    }

    #[test]
    fn revert_keeps_the_compaction_window() {
        let conversation = Conversation::default()
            .push_new(ConversationItem::user("a"))
            .compact("summary".into())
            .push_new(ConversationItem::user("b"))
            .push_new(ConversationItem::user("c"));

        let reverted = conversation.revert(1).unwrap();

        assert_eq!(
            visible(&reverted),
            [
                ConversationItem::System,
                ConversationItem::user("summary"),
                ConversationItem::user("b"),
            ]
        );
        assert!(
            conversation.revert(4).is_err(),
            "must not drop the last visible item"
        );
    }

    #[test]
    fn revert_refuses_to_drop_everything() {
        let conversation = Conversation::default();
        assert!(conversation.revert(1).is_err());
        // used to underflow `len - drop_nums` instead of erroring
        assert!(conversation.revert(2).is_err());
    }
}
