use std::fmt;

use serde::{Deserialize, Serialize, de::Visitor};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PluginSource {
    SelfHosted { name: String },
    Github { repo: String },
}

impl PluginSource {
    pub fn name(&self) -> &str {
        match self {
            Self::SelfHosted { name } => name,
            Self::Github { repo } => repo.split('/').nth(1).unwrap_or(repo),
        }
    }
}

impl fmt::Display for PluginSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfHosted { name } => write!(f, "{name}"),
            Self::Github { repo } => write!(f, "github:{repo}"),
        }
    }
}

impl Serialize for PluginSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PluginSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PluginSourceVisitor;
        impl<'de> Visitor<'de> for PluginSourceVisitor {
            type Value = PluginSource;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if !v.contains(':') {
                    return Ok(PluginSource::SelfHosted { name: v.to_owned() });
                }

                let (variant, content) = v.split_once(':').unwrap();
                let content = content.to_owned();

                match variant {
                    "self-hosted" => Ok(PluginSource::SelfHosted { name: content }),
                    "github" => Ok(PluginSource::Github { repo: content }),
                    variant => Err(E::unknown_variant(variant, &["self-hosted", "github"])),
                }
            }
        }
        deserializer.deserialize_str(PluginSourceVisitor)
    }
}
