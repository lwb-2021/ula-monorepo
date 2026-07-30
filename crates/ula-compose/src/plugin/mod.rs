mod errors;

pub use errors::{PluginError, PluginResult};

use std::future::Future;
use std::io::ErrorKind;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use asynchronous_codec::{FramedWrite, LinesCodec};
use futures_util::SinkExt;
use smol::channel;
use smol::io::{AsyncBufReadExt, BufReader};
use smol::{Async, future};
use ula_protocol::plugin::{PluginInput, PluginMeta, PluginOutput, PluginRequestResponse};

use crate::PATH;
use crate::permission::Ask;
use crate::utils::{ChildGuard, drain_stderr};
use crate::utils::{LineSink, json_line};

use errors::{NotFoundSnafu, TimeoutSnafu};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Per line-operation timeout: a plugin that stops talking is a dead plugin.
/// Waiting on the user's permission answer is deliberately not covered by this.
const IO_TIMEOUT: Duration = Duration::from_secs(120);
/// Upper bound on permission prompts per call, so a misbehaving plugin cannot spin us forever.
const MAX_PERMISSION_REQUESTS: usize = 8;

pub struct PluginState {
    name: String,
    stdin: LineSink<Async<ChildStdin>>,
    stdout: BufReader<Async<ChildStdout>>,
    child: ChildGuard,
}

impl PluginState {
    pub fn start(name: String) -> PluginResult<Self> {
        let mut child: Child = match Command::new(&name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", PATH.to_os_string())
            .spawn()
        {
            Ok(child) => child,
            Err(e) if e.kind() == ErrorKind::NotFound => return NotFoundSnafu { name }.fail(),
            Err(e) => return Err(e.into()),
        };

        if let Some(stderr) = child.stderr.take() {
            drain_stderr(stderr, "plugin");
        }

        let mut child = ChildGuard(child);
        let stdin = Async::new(child.0.stdin.take().expect("piped stdin"))?;
        let stdout = Async::new(child.0.stdout.take().expect("piped stdout"))?;

        Ok(Self {
            name,
            stdin: FramedWrite::new(stdin, LinesCodec),
            stdout: BufReader::new(stdout),
            child,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Asks the plugin for the system prompt it wants to contribute.
    pub async fn handshake(&mut self) -> PluginResult<Option<String>> {
        with_timeout(HANDSHAKE_TIMEOUT, self.handshake_inner()).await
    }

    /// Hands one assistant reply over and waits for what the plugin wants to say back.
    ///
    /// `Ok(None)` means the plugin has nothing to add, so nothing may be echoed back into
    /// the conversation. Transport and protocol failures surface as errors instead of
    /// messages for the same reason.
    pub async fn call(
        &mut self,
        content: &str,
        permission: &channel::Sender<Ask>,
    ) -> PluginResult<Option<String>> {
        self.call_inner(content, permission).await
    }

    pub fn restart(mut self) -> PluginResult<Self> {
        self.child.kill();
        Self::start(self.name)
    }

    async fn handshake_inner(&mut self) -> PluginResult<Option<String>> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line).await? == 0 {
            return Err(std::io::Error::other("plugin closed its stdout").into());
        }
        let meta: PluginMeta = serde_json::from_str(&line.trim())?;
        Ok(meta.system_prompt)
    }

    async fn call_inner(
        &mut self,
        content: &str,
        permission: &channel::Sender<Ask>,
    ) -> PluginResult<Option<String>> {
        with_timeout(
            IO_TIMEOUT,
            self.send(&PluginInput::Message {
                content: content.to_owned(),
            }),
        )
        .await?;

        let mut requests = 0;
        loop {
            match with_timeout(IO_TIMEOUT, self.recv()).await? {
                PluginOutput::SendMessage { message } => return Ok(Some(message)),
                PluginOutput::Idle => return Ok(None),
                PluginOutput::Request {
                    request_type,
                    payload,
                } => {
                    requests += 1;
                    if requests > MAX_PERMISSION_REQUESTS {
                        return Err(
                            std::io::Error::other("plugin asked for permission too often").into(),
                        );
                    }

                    let response =
                        ask_permission(permission, &self.name, request_type, payload).await;
                    with_timeout(IO_TIMEOUT, self.send(&response)).await?;
                }
            }
        }
    }

    async fn send(&mut self, message: &impl serde::Serialize) -> PluginResult<()> {
        self.stdin.send(json_line(message)?).await?;
        Ok(())
    }

    async fn recv(&mut self) -> PluginResult<PluginOutput> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line).await? == 0 {
            return Err(std::io::Error::other("plugin closed its stdout").into());
        }
        Ok(serde_json::from_str(line.trim())?)
    }
}

fn unexpected(what: &str, output: &PluginOutput) -> PluginError {
    std::io::Error::other(format!("expected {what}, got {output:?}")).into()
}

/// Blocks until the user answered the prompt; this wait is not under any timeout.
async fn ask_permission(
    permission: &channel::Sender<Ask>,
    plugin: &str,
    request_type: String,
    payload: std::collections::HashMap<String, String>,
) -> PluginRequestResponse {
    let (reply_tx, reply_rx) = channel::bounded(1);
    let ask = Ask {
        plugin: plugin.to_owned(),
        request_type,
        payload,
        reply: reply_tx,
    };

    if permission.send(ask).await.is_err() {
        return PluginRequestResponse::Rejected {
            reason: Some(String::from("permission prompter is gone")),
        };
    }

    reply_rx
        .recv()
        .await
        .unwrap_or(PluginRequestResponse::Rejected {
            reason: Some(String::from("permission prompter is gone")),
        })
}

async fn with_timeout<T>(
    timeout: Duration,
    op: impl Future<Output = PluginResult<T>>,
) -> PluginResult<T> {
    enum Outcome<T> {
        Done(PluginResult<T>),
        TimedOut,
    }

    let outcome = future::or(async { Outcome::Done(op.await) }, async {
        smol::Timer::after(timeout).await;
        Outcome::TimedOut
    })
    .await;

    match outcome {
        Outcome::Done(result) => result,
        Outcome::TimedOut => TimeoutSnafu.fail(),
    }
}
