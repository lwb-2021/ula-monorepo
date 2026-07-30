use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMeta {
    pub system_prompt: Option<String>,
}

/// Compose -> Plugin (via stdin, one JSON line per message).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginInput {
    Message { content: String },
}

/// Plugin -> Compose (via stdout, one JSON line per message).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginOutput {
    SendMessage {
        message: String,
    },
    Idle,
    Request {
        request_type: String,
        payload: HashMap<String, String>,
    },
}

/// Compose -> Plugin, answering a `PluginOutput::Request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginRequestResponse {
    Allowed,
    Rejected { reason: Option<String> },
    Interrupt,
}
