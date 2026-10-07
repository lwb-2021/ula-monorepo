use anyhow::Context;

use crate::tool::Tool;

pub fn build_lua(tools: &[Tool]) -> anyhow::Result<String> {
    tools
        .iter()
        .map(build_single_lua)
        .collect::<anyhow::Result<Box<[_]>>>()
        .map(|lines| lines.join("\n"))
}

fn build_single_lua(tool: &Tool) -> anyhow::Result<String> {
    let checks = tool
        .meta
        .parameters
        .iter()
        .enumerate()
        .map(|(i, param)| format_checks(i, tool, param))
        .collect::<anyhow::Result<String>>()
        .with_context(|| format!("Invalid parameter format in {}", tool.root.display()))?;
    let signature = tool
        .meta
        .parameters
        .iter()
        .map(|param| {
            let decl = param.split_once('=').unwrap_or((param, "")).0.trim();
            match param_default(param) {
                Some(_) => {
                    format!("{}?", decl.trim())
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
            Some(format!(
                "\t{name} = {name} or ({})\n",
                param_default(param)?
            ))
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

    Ok(format!(
        "function {name}({signature}): string\n{defaults}{checks}\treturn coroutine.yield({{ name = \"{name}\", params = {{ {params} }} }})\nend",
        name = tool.meta.name
    ))
}

fn format_checks(i: usize, tool: &Tool, param: &str) -> anyhow::Result<String> {
    let rest = param
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("No type is available for {param}"))?
        .1;
    let ty = match rest.split_once('=') {
        Some((ty, _default)) => ty,
        None => rest,
    }
    .trim();
    if ty != "string" && ty != "number" && ty != "boolean" {
        anyhow::bail!("Invalid type `{ty}`");
    }
    let name = param_name(param);
    Ok(format!(
        "\tif type({name}) ~= \"{ty}\" then error((\"bad argument #{} to '{}' ({name}: {ty} expected, got %s)\"):format(type({name})), 2) end\n",
        i + 1,
        tool.meta.name
    ))
}

pub fn param_name(param: &str) -> &str {
    param.split([':', '=']).next().unwrap_or(param).trim()
}

pub fn param_default(param: &str) -> Option<&str> {
    param.split_once('=').map(|(_, default)| default.trim())
}
