mod draw;
mod flush;
mod keys;
mod overlay;

use anyhow::Context;
use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::{SinkExt, StreamExt};
use ratatui::{DefaultTerminal, TerminalOptions, Viewport};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use ula_protocol::interface::UIReceive;

use crate::app::{App, Effect};

const INPUT_H: u16 = 3;

pub async fn run(stream: UnixStream) -> anyhow::Result<()> {
    let (_, height) = crossterm::terminal::size().context("unable to read terminal size")?;
    let live = (height / 3).clamp(6, 24);
    let viewport = (live + INPUT_H).min(height).max(INPUT_H);
    let page = viewport.saturating_sub(INPUT_H + 3).max(1);

    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(viewport),
    });
    let result = event_loop(&mut terminal, stream, page).await;
    ratatui::restore();
    result
}

enum Wake {
    Message(UIReceive),
    Key(KeyEvent),
    Ignored,
    Dead,
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    stream: UnixStream,
    page: u16,
) -> anyhow::Result<()> {
    let (reader, writer) = stream.into_split();
    let mut rx = FramedRead::new(reader, LinesCodec::new());
    let mut tx = FramedWrite::new(writer, LinesCodec::new());
    let mut events = EventStream::new();
    let mut app = App::default();
    let mut keys = keys::KeyRouter::default();

    while !app.quit {
        terminal.draw(|frame| draw::render(frame, &mut app))?;

        let wake = tokio::select! {
            message = next_message(&mut rx) => message.map(Wake::Message).unwrap_or(Wake::Dead),
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => Wake::Key(key),
                Some(Ok(_)) => Wake::Ignored,
                Some(Err(err)) => return Err(err.into()),
                None => Wake::Dead,
            },
        };

        let effects = match wake {
            Wake::Dead => {
                log::info!("[tui] stream ended (quit={})", app.quit);
                break;
            }
            Wake::Ignored => continue,
            Wake::Message(message) => app.on_ui(message),
            Wake::Key(key) => app.on_action(keys.route(key, app.modal.is_some()), page),
        };
        apply(terminal, &mut tx, effects).await?;
    }

    Ok(())
}

async fn apply(
    terminal: &mut DefaultTerminal,
    tx: &mut FramedWrite<OwnedWriteHalf, LinesCodec>,
    effects: Vec<Effect>,
) -> anyhow::Result<()> {
    for effect in effects {
        match effect {
            Effect::Send(message) => {
                let json = serde_json::to_string(&message)?;
                tx.send(json).await?;
            }
            Effect::Flush(turn) => flush::turn(terminal, &turn)?,
        }
    }
    Ok(())
}

/// Next parseable message, skipping lines compose should not have sent.
async fn next_message(rx: &mut FramedRead<OwnedReadHalf, LinesCodec>) -> Option<UIReceive> {
    loop {
        let line = match rx.next().await? {
            Ok(line) => line,
            Err(err) => {
                log::warn!("[tui] read error: {err}");
                return None;
            }
        };
        match serde_json::from_str(&line) {
            Ok(message) => return Some(message),
            Err(err) => log::warn!("[tui] bad message ({err}): {line}"),
        }
    }
}
