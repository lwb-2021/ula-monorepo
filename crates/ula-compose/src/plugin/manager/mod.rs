mod plugin;
mod source;
mod utils;

pub use source::PluginSource;

use std::collections::{HashMap, hash_map::Entry};

use eyre::{Result, WrapErr};
use ula_protocol::plugin::{EXCLUSIVE_PLUGIN_CAPABILITIES, PluginCapability};

use crate::plugin::{PluginError, PluginResult, PluginState};

use plugin::Plugin;

pub struct PluginManager {
    plugins: Vec<Plugin>,
    providers: HashMap<PluginCapability, Vec<usize>>,
}

impl PluginManager {
    pub fn load(sources: &[PluginSource]) -> Result<Self> {
        let mut manager = Self {
            plugins: Vec::new(),
            providers: HashMap::new(),
        };

        for source in sources {
            let plugin = Plugin::parse(source)
                .wrap_err_with(|| format!("failed to load plugin `{source}`"))?;
            manager.register(plugin)?;
        }

        Ok(manager)
    }

    pub fn spawn(self) -> Result<Vec<PluginState>> {
        self.plugins
            .into_iter()
            .map(|plugin| {
                let name = plugin.manifest.name.clone();
                PluginState::start(name.clone(), plugin.exec)
                    .wrap_err_with(|| format!("failed to start plugin `{name}`"))
            })
            .collect()
    }

    /// Validates before committing, so a conflict leaves no half-registered plugin.
    fn register(&mut self, plugin: Plugin) -> PluginResult<()> {
        for capability in &plugin.manifest.provides {
            if EXCLUSIVE_PLUGIN_CAPABILITIES.contains(capability)
                && self.providers.contains_key(capability)
            {
                let name1 = self.plugins[self.providers[capability][0]]
                    .manifest
                    .name
                    .clone();
                return Err(PluginError::Conflict {
                    name1,
                    name2: plugin.manifest.name.clone(),
                    capability: *capability,
                });
            }
        }

        let id = self.plugins.len();
        let provides = plugin.manifest.provides.clone();
        self.plugins.push(plugin);
        for capability in provides {
            match self.providers.entry(capability) {
                Entry::Occupied(mut entry) => entry.get_mut().push(id),
                Entry::Vacant(entry) => {
                    entry.insert(vec![id]);
                }
            }
        }
        Ok(())
    }
}
