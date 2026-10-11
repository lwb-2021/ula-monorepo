use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum MessageEventType {
    Streaming = 0,
    Content = 1,
    FullConversation = 2,
}

/// Payload of both `EventFromCore::Message` and `UIReceive::Message`;
/// `message_type` tells the UI whether to stream it live or flush a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEvent {
    pub reasoning: Option<String>,
    pub content: String,
    pub message_type: MessageEventType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserInteraction {
    Text { text: String },
    ExtraSystemPrompt { prompt: String },
    Interrupt,
}
