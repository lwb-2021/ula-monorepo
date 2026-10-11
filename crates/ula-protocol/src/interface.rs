use serde::{Deserialize, Serialize};

use crate::universal::{MessageEvent, UserInteraction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UIReceive {
    Message {
        message: MessageEvent,
    },
    Error {
        error: String,
    },

    Request {
        request_id: u8,
        request_type: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UISend {
    UserInteraction {
        interaction: UserInteraction,
    },

    RequestResult {
        request_id: u8,
        allow: bool,
        reason: Option<String>,
    },
}
