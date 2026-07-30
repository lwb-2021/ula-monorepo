use ula_protocol::plugin::PluginCapability;

#[derive(Debug, snafu::Snafu)]
#[snafu(visibility(pub(super)))]
pub enum PluginError {
    #[snafu(display(
        "plugins `{name1}` and `{name2}` both provide {capability:?}; only one implementation can be present"
    ))]
    Conflict {
        name1: String,
        name2: String,
        capability: PluginCapability,
    },

    #[snafu(display("plugin `{name}` not found"))]
    NotFound { name: String },

    #[snafu(context(false))]
    #[snafu(display("failed to talk to the plugin: {source}"))]
    Communication { source: std::io::Error },

    #[snafu(context(false))]
    #[snafu(display("the plugin sent an unparsable reply: {source}"))]
    Protocol { source: serde_json::Error },

    #[snafu(display("the plugin did not reply in time"))]
    Timeout,
}

pub type PluginResult<T> = Result<T, PluginError>;
