use crate::adapter::Session;
use crate::SYSTEM_PROMPT;
use async_notify::Notify;
use asynchronous_codec::{FramedRead, LinesCodec};
use log::info;
use smol::stream::StreamExt;
use std::{
    io::stdin,
    num::NonZero,
    ops::ControlFlow,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tap::Pipe;
use ula_protocol::core::UserMessage;

pub async fn main_loop(model: String) -> eyre::Result<()> {
    let exit = Arc::new(Notify::new());

    let interrupt_flag = Arc::new(AtomicBool::new(false));
    let session = Arc::new(Session::create(model, interrupt_flag.clone()).await?);

    smol::spawn({
        let session = session.clone();
        let exit = exit.clone();
        async move {
            smol::Unblock::new(stdin())
                .pipe(|stream| FramedRead::new(stream, LinesCodec))
                .map(|line| {
                    line.map_err(anyhow::Error::from)
                        .and_then(|s| serde_json::from_str(&s).map_err(Into::into))
                })
                .try_for_each(|line| match line? {
                    UserMessage::Text { text } => {
                        session.queue_message(text);
                        Ok(())
                    }
                    UserMessage::SystemPrompt { prompt } => {
                        extend_system_prompt(&prompt);
                        Ok(())
                    }
                    UserMessage::Interrupt => {
                        session.interrupt();
                        Ok(())
                    }
                })
                .await
                .inspect_err(|_: &anyhow::Error| notify_exit(&exit))
                .unwrap();
            notify_exit(&exit)
        }
    })
    .detach();

    loop {
        let flow = smol::future::or(
            async {
                exit.notified().await;
                interrupt_flag.store(true, Ordering::Release);
                session.save()?;

                Ok::<_, eyre::Error>(ControlFlow::Break(()))
            },
            async {
                session.poll().await.inspect_err(|_| notify_exit(&exit))?;

                Ok::<_, eyre::Error>(ControlFlow::Continue(()))
            },
        )
        .await?;

        if flow.is_break() {
            break;
        }
    }

    Ok(())
}

#[inline]
fn notify_exit(exit: &Arc<Notify>) {
    (1..Arc::strong_count(&exit)).for_each(|_| exit.notify_n(NonZero::new(1).unwrap()));
}

fn extend_system_prompt(extra: &str) {
    SYSTEM_PROMPT.rcu(|current| Arc::new(format!("{}\n\n{extra}", &**current)));
    info!("system prompt extended by {} bytes", extra.len());
}
