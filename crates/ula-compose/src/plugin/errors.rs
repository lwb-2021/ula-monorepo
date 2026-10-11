use thiserror::Error;
use ula_protocol::plugin::PluginCapability;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error(
        "plugins `{name1}` and `{name2}` both provide {capability:?}; only one implementation can be present"
    )]
    Conflict {
        name1: String,
        name2: String,
        capability: PluginCapability,
    },

    #[error("plugin `{name}` not found")]
    NotFound { name: String },

    #[error("failed to talk to the plugin: {source}")]
    Communication {
        #[from]
        source: std::io::Error,
    },

    #[error("the plugin sent an unparsable reply: {source}")]
    Protocol {
        #[from]
        source: serde_json::Error,
    },

    #[error("the plugin did not reply in time")]
    Timeout,
}

pub type PluginResult<T> = Result<T, PluginError>;

impl PluginError {
    pub fn raise<T>(self) -> Result<T, Self> {
        Err(self)
    }
}
