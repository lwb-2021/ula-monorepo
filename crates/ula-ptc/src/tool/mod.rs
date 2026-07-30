use std::{
    collections::HashMap,
    fs::{self, File, Permissions},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::LazyLock,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

static ULA_TOOLS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .expect("Unable to get home dir")
        .join(".agents/tools")
});

#[derive(Debug, rust_embed::Embed)]
#[folder = "src/tool/builtins"]
struct BuiltinTools;

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolMeta {
    name: String,
    description: String,
    parameters: Box<[String]>,
    exec: PathBuf,
    #[serde(default)]
    inherit_cwd: bool,
}

#[derive(Debug)]
pub struct Tool {
    root: PathBuf,
    meta: ToolMeta,
}

pub fn list_tools() -> anyhow::Result<Vec<Tool>> {
    install_builtin_tools().context("When installing builtin tools")?;
    fs::read_dir(ULA_TOOLS_ROOT.as_path())
        .context("Failed to read $HOME/.agents/tools")?
        .filter_map(|r| {
            r.context("When loading tools")
                .and_then(|entry| {
                    entry
                        .file_type()?
                        .is_dir()
                        .then(|| {
                            File::open(entry.path().join("meta.json"))
                                .with_context(|| {
                                    format!("Failed to load metadata of {}", entry.path().display())
                                })
                                .and_then(|file| {
                                    Ok(Tool {
                                        root: entry.path().to_path_buf(),
                                        meta: serde_json::from_reader(file)
                                            .context("Failed to deserialize")?,
                                    })
                                })
                        })
                        .transpose()
                })
                .transpose()
        })
        .collect()
}

pub fn build_prompt(tools: &[Tool]) -> String {
    tools
        .iter()
        .map(|tool| {
            format!(
                "- {}({}): string\n  {}",
                tool.meta.name,
                tool.meta.parameters.join(", "),
                tool.meta.description
            )
        })
        .collect::<Box<[_]>>()
        .join("\n")
}

pub fn build_lua(tools: &[Tool]) -> String {
    tools
        .iter()
        .map(|tool| {
            let signature = tool
                .meta
                .parameters
                .iter()
                .map(|param| {
                    let decl = param.split('=').next().unwrap_or(param).trim();
                    match (decl.split_once(':'), param_default(param)) {
                        (Some((name, ty)), Some(_)) => {
                            format!("{}: {}?", name.trim(), ty.trim())
                        }
                        _ => decl.to_string(),
                    }
                })
                .collect::<Box<[_]>>()
                .join(", ");
            let defaults = tool
                .meta
                .parameters
                .iter()
                .filter_map(|param| {
                    let name = param_name(param);
                    Some(format!("\t{name} = {name} or ({})\n", param_default(param)?))
                })
                .collect::<String>();
            let params = tool
                .meta
                .parameters
                .iter()
                .map(|param| {
                    let name = param_name(param);
                    format!("{name} = {name}")
                })
                .collect::<Box<[_]>>()
                .join(", ");
            format!(
                "function {name}({signature}): string\n{defaults}\treturn coroutine.yield({{ name = \"{name}\", params = {{ {params} }} }})\nend",
                name = tool.meta.name
            )
        })
        .collect::<Box<[_]>>()
        .join("\n")
}

fn param_name(param: &str) -> &str {
    param.split([':', '=']).next().unwrap_or(param).trim()
}

fn param_default(param: &str) -> Option<&str> {
    param.split_once('=').map(|(_, default)| default.trim())
}

impl Tool {
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn call(&self, payload: &HashMap<String, String>) -> anyhow::Result<String> {
        self.meta
            .parameters
            .iter()
            .map(|param| {
                let name = param_name(param);
                payload
                    .get(name)
                    .cloned()
                    .or_else(|| param_default(param).map(str::to_string))
                    .with_context(|| {
                        format!("Missing parameter `{name}` of tool {}", self.meta.name)
                    })
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .and_then(|args| self.execute(args))
    }

    pub fn execute(&self, parameters: Vec<String>) -> anyhow::Result<String> {
        let mut command = Command::new(self.root.join(self.meta.exec.as_path()));
        if !self.meta.inherit_cwd {
            command.current_dir(&self.root);
        }
        let output = command
            .args(parameters)
            .output()
            .with_context(|| format!("Unable to execute tool {}", self.meta.name))?;
        let mut result = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            result.push_str(&String::from_utf8_lossy(&output.stderr));
            result.push_str(&format!("{}\n", output.status));
        }
        Ok(result)
    }
}

fn install_builtin_tools() -> anyhow::Result<()> {
    BuiltinTools::iter()
        .map(|path| (ULA_TOOLS_ROOT.join(path.as_ref()), path))
        .filter(|(dst, _)| !dst.exists())
        .try_for_each(|(dst, path)| {
            let file = BuiltinTools::get(path.as_ref())
                .expect("SAFETY: path is from BuiltinTools::iter()");

            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }

            fs::write(&dst, file.data.as_ref())
                .with_context(|| format!("Failed to write {}", dst.display()))?;
            fs::set_permissions(&dst, Permissions::from_mode(0o755))
                .with_context(|| format!("Failed to set permission of {}", dst.display()))?;
            Ok(())
        })
}
