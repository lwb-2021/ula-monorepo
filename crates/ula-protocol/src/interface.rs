use serde::{Deserialize, Serialize};

/// TUI/WebUI -> Compose (via Unix Domain Socket)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UISend {
    UserMessage {
        text: String,
    },
    RequestResult {
        request_id: u8,
        allow: bool,
        reason: Option<String>,
    },
    Interrupt,
}

/// Compose -> TUI/WebUI (via Unix Domain Socket)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UIReceive {
    Stream {
        reasoning: Option<String>,
        content: Option<String>,
    },
    Content {
        reasoning: Option<String>,
        content: String,
    },
    Error {
        message: String,
    },

    Request {
        request_id: u8,
        request_type: String,
        message: String,
    },
}
