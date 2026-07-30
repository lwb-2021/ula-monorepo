use std::collections::HashMap;

use log::info;
use smol::channel;
use ula_protocol::interface::UIReceive;
use ula_protocol::plugin::PluginRequestResponse;

/// A plugin asking compose for permission to run a tool.
pub struct Ask {
    pub plugin: String,
    pub request_type: String,
    pub payload: HashMap<String, String>,
    pub reply: channel::Sender<PluginRequestResponse>,
}

/// The UI's verdict on a pending `Ask`, routed by `request_id`.
pub struct Answer {
    pub request_id: u8,
    pub allow: bool,
    pub reason: Option<String>,
}

/// Serializes permission prompts: one modal at a time, answered by id.
pub async fn prompter(
    asks: channel::Receiver<Ask>,
    answers: channel::Receiver<Answer>,
    to_ui: channel::Sender<UIReceive>,
) {
    let mut next_id: u8 = 0;

    while let Ok(ask) = asks.recv().await {
        let request_id = next_id;
        next_id = next_id.wrapping_add(1);

        info!(
            "[perm] {} asks for `{}` {payload:?}",
            ask.plugin,
            ask.request_type,
            payload = ask.payload
        );

        let request = UIReceive::Request {
            request_id,
            request_type: ask.request_type.clone(),
            message: render(&ask.payload),
        };

        if to_ui.send(request).await.is_err() {
            reject(&ask, "the UI is gone").await;
            return;
        }

        loop {
            match answers.recv().await {
                Ok(answer) if answer.request_id == request_id => {
                    let response = if answer.allow {
                        PluginRequestResponse::Allowed
                    } else {
                        PluginRequestResponse::Rejected {
                            reason: answer.reason.or_else(|| Some(String::from("rejected"))),
                        }
                    };
                    let _ = ask.reply.send(response).await;
                    break;
                }
                Ok(_) => continue,
                Err(_) => {
                    reject(&ask, "the UI stopped answering").await;
                    return;
                }
            }
        }
    }
}

async fn reject(ask: &Ask, reason: &str) {
    let _ = ask
        .reply
        .send(PluginRequestResponse::Rejected {
            reason: Some(reason.to_owned()),
        })
        .await;
}

fn render(payload: &HashMap<String, String>) -> String {
    let mut entries: Vec<_> = payload.iter().collect();
    entries.sort();
    entries
        .into_iter()
        .map(|(key, value)| format!("{key} = {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}
