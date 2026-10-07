mod config;
mod core;
mod interface;
mod permission;
mod plugin;
mod utils;

use std::env::{self, args};
use std::ffi::OsString;
use std::fs::File;
use std::future::Future;
use std::io::{Write, stderr};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{ChildStdin, ChildStdout};
use std::sync::LazyLock;

use asynchronous_codec::{FramedWrite, LinesCodec};
use eyre::WrapErr;
use futures_util::{SinkExt, future::join_all};
use log::{error, info, warn};
use smol::io::{AsyncBufReadExt, BufReader};
use smol::{Async, channel, future, stream::StreamExt};
use ula_protocol::core::{EventFromCore, UserMessage};
use ula_protocol::interface::{UIReceive, UISend};

use crate::config::ComposeConfig;
use crate::core::Core;
use crate::interface::Interface;
use crate::plugin::{PluginManager, PluginState};
use crate::utils::{LineSink, json_line};

pub static PATH: LazyLock<OsString> = LazyLock::new(|| {
    env::join_paths(
        [env::current_exe()
            .expect("Unable to get current exe path")
            .parent()
            .expect("Unable to get current exe dir")
            .to_path_buf()]
        .into_iter()
        .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap()
});

type Task = Pin<Box<dyn Future<Output = ()>>>;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .target(env_logger::Target::Pipe(
            File::create("/tmp/ula.log")
                .map(|x| Box::new(x) as Box<dyn Write + Send + 'static>)
                .unwrap_or_else(|_| Box::new(stderr()) as Box<dyn Write + Send + 'static>),
        ))
        .init();

    let config_path = args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("example-config.toml"))
            }
            #[cfg(not(debug_assertions))]
            None
        })
        .expect("No config provided");

    let config: ComposeConfig =
        toml::from_str(&std::fs::read_to_string(&config_path).expect("Failed to read config"))
            .expect("Failed to parse config");

    if let Err(e) = run(config) {
        error!("[compose] startup failed: {e:#}");
        eprintln!("ula-compose: {e:#}");
        std::process::exit(1);
    }
}

fn run(config: ComposeConfig) -> eyre::Result<()> {
    let model = config
        .default_model()
        .expect("No default agent model configured");

    let mut plugins = PluginManager::load(&config.plugins)?.spawn()?;

    let mut core = Core::spawn(model).expect("Failed to spawn ula-core");

    let reason = smol::block_on(async {
        let prompt = collect_system_prompts(&mut plugins).await?;
        inject_system_prompt(&mut core.stdin, &prompt)
            .await
            .wrap_err("failed to inject the plugin system prompt into ula-core")?;

        let ui = Interface::start(config.interface).expect("Failed to spawn interface");
        Ok::<String, eyre::Report>(routing(core, ui, plugins).await)
    })?;

    warn!("[compose] shutdown: {reason}");
    Ok(())
}

async fn collect_system_prompts(plugins: &mut [PluginState]) -> eyre::Result<String> {
    let mut prompts = Vec::new();

    for plugin in plugins.iter_mut() {
        let name = plugin.name().to_owned();
        let outcome = plugin.handshake().await;
        let prompt = outcome.wrap_err_with(|| format!("plugin `{name}` handshake failed"))?;

        if let Some(prompt) = prompt {
            info!(
                "[compose] {name} injects {} bytes of system prompt",
                prompt.len()
            );
            prompts.push(prompt);
        }
    }

    Ok(prompts.join("\n\n"))
}

async fn inject_system_prompt(stdin: &mut Async<ChildStdin>, prompt: &str) -> std::io::Result<()> {
    if prompt.is_empty() {
        return Ok(());
    }

    let message = UserMessage::SystemPrompt {
        prompt: prompt.to_owned(),
    };
    let mut sink = FramedWrite::new(stdin, LinesCodec);
    sink.send(json_line(&message)?).await
}

/// Routes core -> UI, UI -> core and core -> plugin -> core until something fatal happens.
async fn routing(core: Core, ui: Interface, plugins: Vec<PluginState>) -> String {
    let Core {
        stdin,
        stdout,
        child: _core,
    } = core;

    let (mut events_tx, events_rx) = async_broadcast::broadcast::<EventFromCore<'static>>(64);
    events_tx.set_overflow(true);
    events_tx.set_await_active(false);

    let (to_ui_tx, to_ui_rx) = channel::bounded::<UIReceive>(1024);
    let (from_ui_tx, from_ui_rx) = channel::bounded::<UISend>(256);
    let (to_core_tx, to_core_rx) = channel::bounded::<UserMessage>(256);
    let (shutdown_tx, shutdown_rx) = channel::bounded::<String>(8);
    let (ask_tx, ask_rx) = channel::bounded::<permission::Ask>(8);
    let (answer_tx, answer_rx) = channel::bounded::<permission::Answer>(8);

    let mut jobs = Vec::new();
    let mut workers: Vec<Task> = Vec::new();
    for plugin in plugins {
        let (job_tx, job_rx) = channel::bounded::<String>(64);
        jobs.push(job_tx);
        workers.push(Box::pin(plugin_worker(
            plugin,
            job_rx,
            to_core_tx.clone(),
            ask_tx.clone(),
        )));
    }

    let mut tasks: Vec<Task> = vec![
        Box::pin(core_reader(
            stdout,
            events_tx,
            to_ui_tx.clone(),
            shutdown_tx.clone(),
        )),
        Box::pin(ui_link(ui, to_ui_rx, from_ui_tx, shutdown_tx.clone())),
        Box::pin(dispatch(
            events_rx,
            jobs,
            to_ui_tx.clone(),
            shutdown_tx.clone(),
        )),
        Box::pin(ui_messages(
            from_ui_rx,
            to_core_tx.clone(),
            answer_tx.clone(),
        )),
        Box::pin(permission::prompter(ask_rx, answer_rx, to_ui_tx.clone())),
        Box::pin(core_writer(
            to_core_rx,
            FramedWrite::new(stdin, LinesCodec),
            to_ui_tx.clone(),
            shutdown_tx.clone(),
        )),
    ];
    tasks.extend(workers);

    drop(to_ui_tx);
    drop(to_core_tx);
    drop(shutdown_tx);
    drop(ask_tx);
    drop(answer_tx);

    future::or(
        async move {
            join_all(tasks).await;
            String::from("all routing tasks finished")
        },
        async move {
            shutdown_rx
                .recv()
                .await
                .unwrap_or_else(|_| String::from("all routing tasks finished"))
        },
    )
    .await
}

/// Logs a fatal routing problem, shows it in the UI when the UI is still there, and wakes `routing`.
async fn fail(
    shutdown: &channel::Sender<String>,
    to_ui: &channel::Sender<UIReceive>,
    reason: impl Into<String>,
) {
    let reason = reason.into();
    warn!("[compose] {reason}");
    let _ = to_ui.try_send(UIReceive::Error {
        message: reason.clone(),
    });
    let _ = shutdown.send(reason).await;
}

async fn core_reader(
    stdout: Async<ChildStdout>,
    events: async_broadcast::Sender<EventFromCore<'static>>,
    to_ui: channel::Sender<UIReceive>,
    shutdown: channel::Sender<String>,
) {
    let mut lines = BufReader::new(stdout).lines();

    while let Some(line) = lines.next().await {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                fail(
                    &shutdown,
                    &to_ui,
                    format!("ula-core stdout became unreadable: {e}"),
                )
                .await;
                return;
            }
        };

        let event = match serde_json::from_str::<EventFromCore>(&line) {
            Ok(event) => event.into_owned(),
            Err(e) => {
                warn!("[core] ignoring malformed event: {e}");
                continue;
            }
        };

        if events.broadcast(event).await.is_err() {
            return;
        }
    }

    events.close();
    fail(&shutdown, &to_ui, "ula-core exited").await;
}

async fn dispatch(
    mut events: async_broadcast::Receiver<EventFromCore<'static>>,
    jobs: Vec<channel::Sender<String>>,
    to_ui: channel::Sender<UIReceive>,
    shutdown: channel::Sender<String>,
) {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(async_broadcast::RecvError::Overflowed(skipped)) => {
                warn!("[compose] UI stalled, dropped {skipped} core event(s)");
                continue;
            }
            Err(async_broadcast::RecvError::Closed) => return,
        };

        match event {
            EventFromCore::Streaming(msg) => {
                let reasoning = msg.reasoning.map(|reasoning| reasoning.into_owned());
                let content = (!msg.content.is_empty()).then(|| msg.content.into_owned());
                let _ = to_ui.try_send(UIReceive::Stream { reasoning, content });
            }
            EventFromCore::Content(msg) => {
                let reasoning = msg.reasoning.map(|reasoning| reasoning.into_owned());
                let content = msg.content.into_owned();

                let forwarded = to_ui.try_send(UIReceive::Content {
                    reasoning,
                    content: content.clone(),
                });
                if forwarded.is_err() {
                    fail(&shutdown, &to_ui, "UI stopped consuming messages").await;
                    return;
                }

                info!("[compose] content ({} bytes) → plugin", content.len());

                if jobs.is_empty() {
                    warn!("[compose] no plugin is listening for tool calls");
                    continue;
                }

                for job in &jobs {
                    if job.send(content.clone()).await.is_err() {
                        error!("[compose] a plugin disappeared, dropping its reply");
                    }
                }
            }
            EventFromCore::Error { message } => {
                warn!("[compose] core error: {message}");
                let _ = to_ui.try_send(UIReceive::Error {
                    message: message.into_owned(),
                });
            }
            // Reserved for session resume / context management, see `EventFromCore`.
            EventFromCore::FullConversation(_) => {}
        }
    }
}

async fn ui_link(
    ui: Interface,
    to_ui_rx: channel::Receiver<UIReceive>,
    from_ui_tx: channel::Sender<UISend>,
    shutdown: channel::Sender<String>,
) {
    let stream = match ui.accept().await {
        Ok(stream) => stream,
        Err(e) => {
            let _ = shutdown
                .send(format!("unable to accept the UI connection: {e}"))
                .await;
            return;
        }
    };
    info!("[ui] connected");

    let reader = {
        let mut lines = BufReader::new(stream.clone()).lines();
        let from_ui_tx = from_ui_tx;
        let shutdown = shutdown.clone();
        async move {
            while let Some(line) = lines.next().await {
                match line {
                    Ok(line) => match serde_json::from_str::<UISend>(&line) {
                        Ok(message) => {
                            if from_ui_tx.send(message).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => warn!("[ui] ignoring malformed message: {e}"),
                    },
                    Err(e) => warn!("[ui] read failed: {e}"),
                }
            }

            info!("[ui] disconnected");
            let _ = shutdown.send(String::from("UI disconnected")).await;
        }
    };

    let writer = {
        let mut sink = FramedWrite::new(stream, LinesCodec);
        let mut lines = Box::pin(to_ui_rx.map(|message| json_line(&message)));
        async move {
            if let Err(e) = sink.send_all(&mut lines).await {
                let _ = shutdown
                    .send(format!("unable to write to the UI: {e}"))
                    .await;
            }
        }
    };

    future::zip(reader, writer).await;
}

async fn ui_messages(
    from_ui: channel::Receiver<UISend>,
    to_core: channel::Sender<UserMessage>,
    answers: channel::Sender<permission::Answer>,
) {
    while let Ok(message) = from_ui.recv().await {
        let message = match message {
            UISend::UserMessage { text } => UserMessage::Text { text },
            UISend::Interrupt => UserMessage::Interrupt,
            UISend::RequestResult {
                request_id,
                allow,
                reason,
            } => {
                let answer = permission::Answer {
                    request_id,
                    allow,
                    reason,
                };
                if answers.send(answer).await.is_err() {
                    return;
                }
                continue;
            }
        };

        if to_core.send(message).await.is_err() {
            return;
        }
    }
}

async fn core_writer(
    to_core: channel::Receiver<UserMessage>,
    mut sink: LineSink<Async<ChildStdin>>,
    to_ui: channel::Sender<UIReceive>,
    shutdown: channel::Sender<String>,
) {
    let mut lines = Box::pin(to_core.map(|message| json_line(&message)));

    if let Err(e) = sink.send_all(&mut lines).await {
        fail(
            &shutdown,
            &to_ui,
            format!("unable to write to ula-core: {e}"),
        )
        .await;
    }
}

async fn plugin_worker(
    mut plugin: PluginState,
    jobs: channel::Receiver<String>,
    to_core: channel::Sender<UserMessage>,
    permission: channel::Sender<permission::Ask>,
) {
    let name = plugin.name().to_owned();

    while let Ok(content) = jobs.recv().await {
        let reply = match plugin.call(&content, &permission).await {
            Ok(reply) => reply,
            Err(e) => {
                error!("[plugin] {name}: {e}, restarting");
                match plugin.restart() {
                    Ok(restarted) => {
                        plugin = restarted;
                        continue;
                    }
                    Err(e) => {
                        error!("[plugin] {name}: unable to restart: {e}");
                        return;
                    }
                }
            }
        };

        let Some(message) = reply else {
            info!("[plugin] {name}: idle");
            continue;
        };

        info!("[plugin] {name}: {} bytes → core", message.len());

        if to_core
            .send(UserMessage::Text { text: message })
            .await
            .is_err()
        {
            return;
        }
    }
}
