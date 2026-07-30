use std::env;

use anyhow::Context;
use tokio::net::UnixStream;

mod app;
mod modal;
mod tui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .target(env_logger::Target::Stderr)
        .init();

    let socket = env::args()
        .nth(1)
        .context("usage: ula-tui <socket-path>")?;
    let stream = UnixStream::connect(&socket)
        .await
        .with_context(|| format!("unable to connect to socket {socket}"))?;

    let result = tui::run(stream).await;
    match &result {
        Ok(()) => log::info!("[tui] exiting cleanly"),
        Err(e) => log::error!("[tui] exiting with error: {e:#}"),
    }
    result
}
