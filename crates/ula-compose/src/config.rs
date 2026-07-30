use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ComposeConfig {
    pub(super) agents: Vec<AgentConfig>,
    pub(super) plugins: Vec<String>,
    pub(super) interface: String,
}

impl ComposeConfig {
    pub(super) fn default_model(&self) -> Option<&str> {
        self.agents
            .iter()
            .find(|agent| agent.default)
            .or_else(|| self.agents.first())?
            .model
            .as_deref()
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentConfig {
    name: String,
    model: Option<String>,
    default: bool,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::config::ComposeConfig;

    #[test]
    fn compose_config_deserialize() {
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("example-config.yaml");
        let _: ComposeConfig =
            noyalib::from_str(&fs::read_to_string(&config_dir).unwrap()).unwrap();
    }
}
