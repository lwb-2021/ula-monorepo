mod errors;
mod parser;
mod runtime;
mod tool;

use std::io::{BufRead, BufReader, Write, stdin, stdout};

use anyhow::{Context, anyhow};
use ula_protocol::plugin::{PluginInput, PluginMeta, PluginOutput, PluginRequestResponse};

use crate::runtime::{LuaEvent, Runtime};
use crate::tool::Tool;

const SYSTEM_PROMPT_PREFIX: &str = include_str!("prompt.md");
const RESULT_BEGIN: &str = "<|TOOLCALL_RESULT|>";
const RESULT_END: &str = "<|END|>";

fn main() -> anyhow::Result<()> {
    let mut transport = BufReader::new(stdin());
    let mut line = String::new();

    let tools = tool::list_tools()?;
    let tools_prompt = tool::build_prompt(&tools);
    let mut runtime = Runtime::new()?;
    runtime.inject(&tool::build_lua(&tools)?)?;

    emit(&PluginMeta {
        system_prompt: Some(format!("{SYSTEM_PROMPT_PREFIX}\n{tools_prompt}")),
    })
    .context("Failed to send metadata")?;

    loop {
        line.clear();
        if transport.read_line(&mut line)? == 0 {
            return Ok(());
        }

        match serde_json::from_str::<PluginInput>(line.trim())? {
            PluginInput::Message { content } => {
                let output = match run(&mut runtime, &mut transport, &tools, &content) {
                    Ok(None) => PluginOutput::Idle,
                    Ok(Some(result)) => PluginOutput::SendMessage {
                        message: wrap(&result),
                    },
                    Err(e) => PluginOutput::SendMessage {
                        message: wrap(&format!("TOOLCALL_FAILURE: {e:#}")),
                    },
                };
                emit(&output)?;
            }
            PluginInput::CompactRequest {
                full_conversation: _,
            } => {
                return Err(anyhow!(
                    "compaction is not supported: `ula-ptc` does not provide the context_manager capability"
                ));
            }
        }
    }
}

fn run(
    runtime: &mut Runtime,
    transport: &mut impl BufRead,
    tools: &[Tool],
    content: &str,
) -> anyhow::Result<Option<String>> {
    let Some(blocks) = parser::extract(content) else {
        return Ok(None);
    };
    blocks
        .iter()
        .map(|code| run_block(runtime, transport, tools, code))
        .collect::<anyhow::Result<Vec<_>>>()
        .map(|results| Some(results.join("\n")))
}

fn run_block(
    runtime: &mut Runtime,
    transport: &mut impl BufRead,
    tools: &[Tool],
    code: &str,
) -> anyhow::Result<String> {
    // eprintln!("[run] {code:?}");
    let mut event = runtime.execute(code)?;
    loop {
        match event {
            LuaEvent::Finished { result } => return Ok(result),
            LuaEvent::ToolCall {
                request_type,
                payload,
            } => {
                emit(&PluginOutput::Request {
                    request_type: request_type.clone(),
                    payload: payload.clone(),
                })?;

                let output = match read_response(transport)? {
                    PluginRequestResponse::Allowed => tools
                        .iter()
                        .find(|tool| tool.name() == request_type)
                        .ok_or_else(|| anyhow!("unknown tool `{request_type}`"))?
                        .call(&payload)?,
                    PluginRequestResponse::Rejected { reason } => {
                        return Err(anyhow!(
                            "PERMISSION_DENIED_BY_USER: {}",
                            reason.unwrap_or_else(|| String::from("no reason provided"))
                        ));
                    }
                    PluginRequestResponse::Interrupt => return Err(anyhow!("INTERRUPTED")),
                };
                event = runtime.resume(&output)?;
            }
        }
    }
}

fn emit(output: &impl serde::Serialize) -> anyhow::Result<()> {
    let mut out = stdout();
    writeln!(out, "{}", serde_json::to_string(output)?)?;
    out.flush()?;
    Ok(())
}

fn read_response(transport: &mut impl BufRead) -> anyhow::Result<PluginRequestResponse> {
    let mut line = String::new();
    if transport.read_line(&mut line)? == 0 {
        return Err(anyhow!("EOF while waiting for a permission response"));
    }
    Ok(serde_json::from_str(line.trim())?)
}

fn wrap(result: &str) -> String {
    format!("{RESULT_BEGIN}\n{result}\n{RESULT_END}")
}
