use std::{ffi::OsStr, io, os::unix::process::CommandExt, path::PathBuf, process::Command};

use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cmd {
    profile: Option<String>,
    #[command(subcommand)]
    subcommand: Option<Subcommand>,
}

#[derive(Clone, clap::Subcommand)]
pub enum Subcommand {
    Compose { file: PathBuf },
}

fn main() {
    let cmd = Cmd::parse();
    let args = match cmd.subcommand {
        None => {
            let profile_dir = xdg::BaseDirectories::with_profile(
                "ula",
                cmd.profile.unwrap_or("default".to_string()),
            );

            profile_dir
                .get_config_file("config.toml")
                .expect("Unable to get config file")
        }
        Some(Subcommand::Compose { file }) => file,
    };
    exec_compose(args).expect("Unable to spawn ula-compose");
}
fn exec_compose(compose_config: impl AsRef<OsStr>) -> io::Result<()> {
    const EXEC: &str = cfg_select! {
        debug_assertions => concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug/ula-compose"),

        _ => "ula-compose"
    };
    Err(Command::new(EXEC).arg(compose_config).exec())
}
