mod errors;
mod parser;
mod runtime;
mod tool;

use std::io::{BufRead, BufReader, Write, stdin, stdout};
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, anyhow};
use ula_protocol::plugin::{PluginInput, PluginMeta, PluginOutput, PluginRequestResponse};

use crate::runtime::{LuaEvent, Runtime};
use crate::tool::Tool;

const SYSTEM_PROMPT_PREFIX: &str = include_str!("prompt.md");
const RESULT_BEGIN: &str = "<|TOOLCALL_RESULT|>";
const RESULT_END: &str = "<|END|>";

static ID: AtomicU32 = AtomicU32::new(0);

fn main() -> anyhow::Result<()> {
    let mut transport = BufReader::new(stdin());
    let mut line = String::new();

    let tools = tool::list_tools()?;
    let tools_prompt = tool::build_prompt(&tools);
    let mut runtime = Runtime::new()?;
    runtime.inject(&tool::build_lua(&tools))?;

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
                        message: wrap(&format!("TOOLCALL_FAILURE: {e}")),
                    },
                };
                emit(&output)?;
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
    parser::extract(content)
        .map(|blocks| {
            blocks
                .iter()
                .map(|code| run_block(runtime, transport, tools, code))
                .collect::<anyhow::Result<Vec<String>>>()
                .map(|result| result.join("\n"))
        })
        .transpose()
}

fn run_block(
    runtime: &mut Runtime,
    transport: &mut impl BufRead,
    tools: &[Tool],
    code: &str,
) -> anyhow::Result<String> {
    eprintln!("[run] {code:?}");

    let id = ID.fetch_add(1, Ordering::Relaxed);
    if let Err(e) = runtime.execute(id, code) {
        return Ok(format!("TOOLCALL_FAILURE: {e}"));
    }

    loop {
        let Some(event) = runtime.poll() else {
            return Ok(String::from("TOOLCALL_FAILURE: nothing left to run"));
        };

        match event {
            LuaEvent::Finished { result, .. } => {
                eprintln!("[done] {result:?}");
                return Ok(result);
            }
            LuaEvent::Error { message, .. } => {
                eprintln!("[fail] {message}");
                return Ok(format!("TOOLCALL_FAILURE: {message}"));
            }
            LuaEvent::ToolCall {
                id,
                request_type,
                payload,
            } => {
                emit(&PluginOutput::Request {
                    request_type: request_type.clone(),
                    payload: payload.clone(),
                })?;

                match read_response(transport)? {
                    PluginRequestResponse::Allowed => {
                        let output = tools
                            .iter()
                            .find(|tool| tool.name() == request_type)
                            .ok_or_else(|| anyhow!("unknown tool `{request_type}`"))
                            .and_then(|tool| tool.call(&payload));
                        match output {
                            Ok(output) => runtime.resume(id, Some(output))?,
                            Err(e) => {
                                runtime.cancel(id);
                                return Ok(format!("TOOLCALL_FAILURE: {e}"));
                            }
                        }
                    }
                    PluginRequestResponse::Rejected { reason } => {
                        runtime.cancel(id);
                        return Ok(format!(
                            "PERMISSION_DENIED_BY_USER: {}",
                            reason.unwrap_or_else(|| String::from("no reason provided"))
                        ));
                    }
                    PluginRequestResponse::Interrupt => {
                        runtime.cancel(id);
                        return Ok(String::from("INTERRUPTED"));
                    }
                }
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
