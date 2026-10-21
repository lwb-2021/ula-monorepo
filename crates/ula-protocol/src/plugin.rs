use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    ContextManager,
}

pub const EXCLUSIVE_PLUGIN_CAPABILITIES: [PluginCapability; 1] = [PluginCapability::ContextManager];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMeta {
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginInput {
    Message { content: String },
    CompactRequest { full_conversation: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginOutput {
    SendMessage {
        message: String,
    },
    Idle,
    Request {
        request_type: String,
        payload: HashMap<String, String>,
    },

    Compact {
        summary: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRequestResponse {
    Allowed,
    Rejected { reason: Option<String> },
    Interrupt,
}
