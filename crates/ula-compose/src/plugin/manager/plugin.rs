use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use eyre::{Result, WrapErr, eyre};
use serde::Deserialize;
use ula_protocol::plugin::PluginCapability;

use super::{
    source::PluginSource,
    utils::{find_installed, install, run_install},
};

static BUILTIN: LazyLock<HashMap<&'static str, PluginManifest>> = LazyLock::new(|| {
    HashMap::from([(
        "ula-ptc",
        PluginManifest {
            name: String::from("ula-ptc"),
            provides: [].into(),
            exec: String::from("ula-ptc"),
            install: None,
        },
    )])
});

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default)]
    pub provides: Box<[PluginCapability]>,
    pub exec: String,
    #[serde(default)]
    pub install: Option<String>,
}

pub struct Plugin {
    pub manifest: PluginManifest,
    pub exec: PathBuf,
}

impl Plugin {
    pub fn parse(source: &PluginSource) -> Result<Self> {
        let (manifest, root) = match find_installed(source.name()) {
            Some(root) => (read_manifest(&root)?, Some(root)),
            None => match source {
                PluginSource::SelfHosted { name } => {
                    let manifest = BUILTIN.get(name.as_str()).cloned().ok_or_else(|| {
                        eyre!(
                            "plugin `{name}` not found: no ula-plugin.toml in any XDG data dir and no builtin with that name"
                        )
                    })?;
                    (manifest, None)
                }
                PluginSource::Github { repo } => {
                    let (root, fresh) = install(repo)?;
                    let manifest = read_manifest(&root)?;
                    if fresh && let Some(command) = &manifest.install {
                        run_install(&root, command)?;
                    }
                    (manifest, Some(root))
                }
            },
        };

        let exec = resolve_exec(&manifest.exec, root.as_deref())?;
        Ok(Self { manifest, exec })
    }
}

fn read_manifest(root: &Path) -> Result<PluginManifest> {
    let path = root.join("ula-plugin.toml");
    let text = std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&text).wrap_err_with(|| format!("invalid manifest {}", path.display()))
}

fn resolve_exec(exec: &str, root: Option<&Path>) -> Result<PathBuf> {
    let path = Path::new(exec);
    if path.is_absolute() {
        return Ok(path.to_owned());
    }

    if let Some(root) = root {
        let candidate = root.join(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    std::env::split_paths(&*crate::PATH)
        .map(|dir| dir.join(path))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| eyre!("executable `{exec}` found neither in the plugin root nor in PATH"))
}
