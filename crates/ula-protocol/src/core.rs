use serde::{Deserialize, Serialize};

use crate::universal::{MessageEvent, UserInteraction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventFromCore {
    Message { message: MessageEvent },
    Error { error_msg: String },
    CompactRequest { full_conversation: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventToCore {
    UserInteraction { interaction: UserInteraction },
    Compact { summary: String },
}
