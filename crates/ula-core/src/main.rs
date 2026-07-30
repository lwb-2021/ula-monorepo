#[cfg(target_family = "windows")]
compile_error!("Windows is strictly forbidden in this codebase. Go touch WSL or get out.");

mod adapter;
mod config;
mod main_loop;

use std::{
    fs::File,
    sync::{Arc, LazyLock},
};

#[cfg(debug_assertions)]
use std::path::PathBuf;

use arc_swap::ArcSwap;
use log::info;

#[cfg(debug_assertions)]
use log::warn;

use crate::config::CONFIG;

pub static SYSTEM_PROMPT: LazyLock<ArcSwap<String>> =
    LazyLock::new(|| ArcSwap::from_pointee(include_str!("system-prompt.md").to_owned()));

fn main() -> eyre::Result<()> {
    smol::block_on(main_inner())
}

async fn main_inner() -> eyre::Result<()> {
    env_logger::builder()
        .target(env_logger::Target::Stderr)
        .filter_level(log::LevelFilter::Debug)
        .init();

    let config_home = dirs::config_dir().unwrap_or_default().join("ula");
    #[allow(unused)]
    let config_file = config_home.join("config.yaml");

    #[cfg(debug_assertions)]
    let config_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("example-config.yaml");

    info!("Using config {}", config_file.display());

    match File::open(&config_file) {
        Ok(reader) => {
            CONFIG.store(Arc::new(noyalib::from_reader(reader)?));
        }
        Err(err) => {
            #[cfg(not(debug_assertions))]
            return Err(eyre::eyre!(
                "Unable to read config {}: {err}",
                config_file.display()
            ));
            #[cfg(debug_assertions)]
            warn!("Unable to read config: {}", err);
        }
    }

    let model = std::env::args()
        .nth(1)
        .ok_or_else(|| eyre::eyre!("ula-core needs a `provider:model` argument"))?;
    main_loop::main_loop(model).await?;

    Ok(())
}
