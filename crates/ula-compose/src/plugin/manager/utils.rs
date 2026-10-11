use std::{
    path::{Path, PathBuf},
    process::Command,
};

use eyre::{OptionExt, Result, WrapErr, bail};

use crate::XDG;

pub(super) fn find_installed(name: &str) -> Option<PathBuf> {
    XDG.find_data_file(format!("plugins/{name}/ula-plugin.toml"))
        .and_then(|manifest| manifest.parent().map(PathBuf::from))
}

/// Returns the plugin root and whether a fresh clone was made.
pub(super) fn install(repo: &str) -> Result<(PathBuf, bool)> {
    let (owner, rest) = repo.split_once('/').ok_or_eyre(format!(
        "invalid github source `{repo}`: expected owner/repo[/ref]"
    ))?;
    let (name, reference) = match rest.split_once('/') {
        Some((name, reference)) => (name, Some(reference)),
        None => (rest, None),
    };

    let root = XDG
        .get_data_home()
        .ok_or_eyre("unable to resolve the XDG data home")?
        .join("plugins")
        .join(name);
    if root.exists() {
        return Ok((root, false));
    }

    let parent = root.parent().expect("plugin root always has a parent");
    std::fs::create_dir_all(parent)
        .wrap_err_with(|| format!("failed to create {}", parent.display()))?;

    let url = format!("https://github.com/{owner}/{name}.git");
    let mut command = Command::new("git");
    command.args(["clone", "--depth", "1"]);
    if let Some(reference) = reference {
        command.args(["--branch", reference]);
    }
    let status = command
        .arg(&url)
        .arg(&root)
        .status()
        .wrap_err("failed to run `git`, is it installed?")?;
    if !status.success() {
        bail!("`git clone {url}` failed with {status}");
    }

    Ok((root, true))
}

pub(super) fn run_install(root: &Path, command: &str) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .status()
        .wrap_err_with(|| format!("failed to run the install command `{command}`"))?;
    if !status.success() {
        bail!("the install command `{command}` failed with {status}");
    }
    Ok(())
}
