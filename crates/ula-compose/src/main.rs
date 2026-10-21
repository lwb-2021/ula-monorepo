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
use futures_util::{SinkExt, StreamExt, future::join_all};
use log::{error, info, warn};
use smol::io::{AsyncBufReadExt, BufReader};
use smol::net::unix::UnixStream;
use smol::{Async, channel, future};
use ula_protocol::core::{EventFromCore, EventToCore};
use ula_protocol::interface::{UIReceive, UISend};
use ula_protocol::universal::{MessageEvent, MessageEventType, UserInteraction};
use xdg::BaseDirectories;

use crate::config::ComposeConfig;
use crate::core::Core;
use crate::interface::Interface;
use crate::plugin::{PluginManager, PluginSource, PluginState};
use crate::utils::{LineSink, json_line};
use ula_protocol::plugin::PluginCapability;

static PATH: LazyLock<OsString> = LazyLock::new(|| {
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

static XDG: LazyLock<BaseDirectories> = LazyLock::new(|| BaseDirectories::with_prefix("ula"));

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

    let config_path = args().nth(1).map(PathBuf::from);

    let config = if let Some(config_path) = config_path {
        toml::from_str(&std::fs::read_to_string(&config_path).expect("Failed to read config"))
            .expect("Failed to parse config")
    } else {
        warn!("Cannot get config, using default one");
        ComposeConfig {
            agents: vec![],
            plugins: vec![PluginSource::SelfHosted {
                name: "ula-ptc".into(),
            }],
            interface: "ula-tui".to_string(),
        }
    };

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

    let message = EventToCore::UserInteraction {
        interaction: UserInteraction::ExtraSystemPrompt {
            prompt: prompt.to_owned(),
        },
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

    let stream = match ui.accept().await {
        Ok(stream) => stream,
        Err(e) => return format!("unable to accept the UI connection: {e}"),
    };
    info!("[ui] connected");

    let (events_tx, events_rx) = async_broadcast::broadcast::<EventFromCore>(64);

    let (to_ui_tx, to_ui_rx) = channel::bounded::<UIReceive>(1024);
    let (to_core_tx, to_core_rx) = channel::bounded::<EventToCore>(256);
    let (shutdown_tx, shutdown_rx) = channel::bounded::<String>(8);
    let (ask_tx, ask_rx) = channel::bounded::<permission::Ask>(8);
    let (answer_tx, answer_rx) = channel::bounded::<permission::Answer>(8);

    let workers: Vec<_> = plugins
        .into_iter()
        .map(|plugin| {
            Box::pin(plugin_worker(
                events_rx.clone(),
                plugin,
                to_core_tx.clone(),
                ask_tx.clone(),
            )) as Task
        })
        .collect();

    let mut tasks: Vec<Task> = vec![
        Box::pin(core_link(
            stdout,
            events_tx,
            to_ui_tx.clone(),
            shutdown_tx.clone(),
        )),
        Box::pin(event_dispatch(
            events_rx,
            to_ui_tx.clone(),
            shutdown_tx.clone(),
        )),
        Box::pin(ui_input(
            stream.clone(),
            to_core_tx.clone(),
            answer_tx.clone(),
            shutdown_tx.clone(),
        )),
        Box::pin(ui_output(to_ui_rx, stream, shutdown_tx.clone())),
        Box::pin(permission::prompter(ask_rx, answer_rx, to_ui_tx.clone())),
        Box::pin(core_output(
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

async fn fail(
    shutdown: &channel::Sender<String>,
    to_ui: &channel::Sender<UIReceive>,
    reason: impl Into<String>,
) {
    let reason = reason.into();
    warn!("[compose] {reason}");
    let _ = to_ui.try_send(UIReceive::Error {
        error: reason.clone(),
    });
    let _ = shutdown.send(reason).await;
}

async fn core_link(
    stdout: Async<ChildStdout>,
    events: async_broadcast::Sender<EventFromCore>,
    to_ui: channel::Sender<UIReceive>,
    shutdown: channel::Sender<String>,
) {
    BufReader::new(stdout)
        .lines()
        .take_while(|line| {
            if let Err(e) = line {
                warn!("[core] ula-core stdout became unreadable: {e}");
            }
            std::future::ready(line.is_ok())
        })
        .filter_map(|line| async move {
            let line = line.ok()?;
            match serde_json::from_str::<EventFromCore>(&line) {
                Ok(event) => Some(event),
                Err(e) => {
                    warn!("[core] ignoring malformed event: {e}");
                    None
                }
            }
        })
        .for_each(|event| async {
            let _ = events.broadcast(event).await;
        })
        .await;

    events.close();
    fail(&shutdown, &to_ui, "ula-core exited").await;
}

async fn event_dispatch(
    events: async_broadcast::Receiver<EventFromCore>,
    to_ui: channel::Sender<UIReceive>,
    shutdown: channel::Sender<String>,
) {
    events
        .for_each(|event| async {
            let message = match event {
                EventFromCore::Message { message } => UIReceive::Message { message },
                EventFromCore::Error { error_msg } => {
                    warn!("[compose] core error: {error_msg}");
                    UIReceive::Error { error: error_msg }
                }
                EventFromCore::CompactRequest { .. } => return,
            };

            if to_ui.send(message).await.is_err() {
                fail(&shutdown, &to_ui, "UI stopped consuming messages").await;
            }
        })
        .await;
}

async fn ui_input(
    stream: UnixStream,
    to_core: channel::Sender<EventToCore>,
    answers: channel::Sender<permission::Answer>,
    shutdown: channel::Sender<String>,
) {
    BufReader::new(stream)
        .lines()
        .filter_map(|line| async move {
            match line {
                Ok(line) => match serde_json::from_str::<UISend>(&line) {
                    Ok(message) => Some(message),
                    Err(e) => {
                        warn!("[ui] ignoring malformed message: {e}");
                        None
                    }
                },
                Err(e) => {
                    warn!("[ui] read failed: {e}");
                    None
                }
            }
        })
        .for_each(|message| async {
            match message {
                UISend::UserInteraction { interaction } => {
                    let _ = to_core
                        .send(EventToCore::UserInteraction { interaction })
                        .await;
                }
                UISend::RequestResult {
                    request_id,
                    allow,
                    reason,
                } => {
                    let _ = answers
                        .send(permission::Answer {
                            request_id,
                            allow,
                            reason,
                        })
                        .await;
                }
            }
        })
        .await;

    info!("[ui] disconnected");
    let _ = shutdown.send(String::from("UI disconnected")).await;
}

async fn ui_output(
    to_ui: channel::Receiver<UIReceive>,
    stream: UnixStream,
    shutdown: channel::Sender<String>,
) {
    let sink = FramedWrite::new(stream, LinesCodec);
    let result = to_ui.map(|message| json_line(&message)).forward(sink).await;
    if let Err(e) = result {
        let _ = shutdown
            .send(format!("unable to write to the UI: {e}"))
            .await;
    }
}

async fn core_output(
    to_core: channel::Receiver<EventToCore>,
    sink: LineSink<Async<ChildStdin>>,
    to_ui: channel::Sender<UIReceive>,
    shutdown: channel::Sender<String>,
) {
    let result = to_core
        .map(|message| json_line(&message))
        .forward(sink)
        .await;
    if let Err(e) = result {
        fail(
            &shutdown,
            &to_ui,
            format!("unable to write to ula-core: {e}"),
        )
        .await;
    }
}

async fn plugin_worker(
    mut events: async_broadcast::Receiver<EventFromCore>,
    mut plugin: PluginState,
    to_core: channel::Sender<EventToCore>,
    permission: channel::Sender<permission::Ask>,
) {
    let name = plugin.name().to_owned();

    while let Some(event) = events.next().await {
        let message = match event {
            EventFromCore::Message {
                message:
                    MessageEvent {
                        content,
                        message_type: MessageEventType::Content,
                        ..
                    },
            } => {
                info!(
                    "[compose] content ({} bytes) → plugin `{name}`",
                    content.len()
                );

                let reply = match plugin.call(&content, &permission).await {
                    Ok(reply) => reply,
                    Err(e) => {
                        error!("[plugin] {name}: {e}, restarting");
                        match plugin.restart().await {
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
                message
            }
            EventFromCore::CompactRequest { full_conversation } => {
                if !plugin.provides(PluginCapability::ContextManager) {
                    continue;
                }

                info!(
                    "[compose] compact request ({} bytes) → plugin `{name}`",
                    full_conversation.len()
                );

                let mut restarted = false;
                loop {
                    match plugin.compact(&full_conversation, &permission).await {
                        Ok(summary) => {
                            info!(
                                "[plugin] {name}: compaction summary ({} bytes)",
                                summary.len()
                            );

                            if to_core
                                .send(EventToCore::Compact { summary })
                                .await
                                .is_err()
                            {
                                return;
                            }
                            break;
                        }
                        Err(e) if restarted => {
                            error!("[plugin] {name}: cannot compact: {e}");
                            return;
                        }
                        Err(e) => {
                            error!("[plugin] {name}: cannot compact: {e}, restarting");
                            match plugin.restart().await {
                                Ok(fresh) => {
                                    plugin = fresh;
                                    restarted = true;
                                }
                                Err(e) => {
                                    error!("[plugin] {name}: unable to restart: {e}");
                                    return;
                                }
                            }
                        }
                    }
                }
                continue;
            }
            _ => continue,
        };

        info!("[plugin] {name}: {} bytes → core", message.len());

        if to_core
            .send(EventToCore::UserInteraction {
                interaction: UserInteraction::Text { text: message },
            })
            .await
            .is_err()
        {
            return;
        }
    }
}
