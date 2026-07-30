use std::{collections::HashMap, sync::LazyLock};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

pub static CONFIG: LazyLock<ArcSwap<Config>> =
    LazyLock::new(|| ArcSwap::from_pointee(Config::default()));

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    Deny,
    Redirect(String),

    #[default]
    Ask,

    Allow,
}

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

    pub providers: HashMap<String, ProviderConfig>,
    // pub permissions: HashMap<String, PermissionAction>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http_timeout: 10,

            providers: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::config::Config;

    #[test]
    fn config_deserialize() {
        let example = include_str!("../example-config.yaml");
        let _config: Config = noyalib::from_str_borrowing(example).unwrap();
    }
}
