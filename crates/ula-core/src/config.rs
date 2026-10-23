use std::{collections::HashMap, sync::LazyLock};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

pub static CONFIG: LazyLock<ArcSwap<Config>> =
    LazyLock::new(|| ArcSwap::from_pointee(Config::default()));

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderConfig {
    pub base_url: String,
    pub endpoint: String,
    pub api_key_command: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub http_timeout: u8,

    pub context_limit: f32,

    pub context_window: Option<u32>,

    pub providers: HashMap<String, ProviderConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http_timeout: 10,

            context_limit: 0.8,

            context_window: None,

            providers: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::config::Config;

    #[test]
    fn config_deserialize() {
        let example = include_str!("../example-config.toml");
        let _config: Config = toml::from_str(example).unwrap();
    }
}
