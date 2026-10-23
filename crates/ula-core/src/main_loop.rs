use crate::adapter::Session;

use std::io::stdin;

use asynchronous_codec::{FramedRead, LinesCodec};
use eyre::Context;
use smol::{channel, stream::StreamExt};
use ula_protocol::core::EventToCore;

pub async fn main_loop(model: String) -> eyre::Result<()> {
    let mut session = Session::create(model).await?;

    let (tx, rx) = channel::unbounded();

    smol::spawn(async move {
        let mut stream = FramedRead::new(smol::Unblock::new(stdin()), LinesCodec);

        while let Some(Ok(item)) = stream.next().await {
            let event: EventToCore =
                serde_json::from_str(&item).context("Invaild event from stdin")?;

            let Ok(()) = tx.send(event).await else {
                break;
            };
        }
        eyre::Ok(())
    })
    .detach();

    session.serve(&rx).await?;

    session.save().context("When saving session")?;

    Ok(())
}
