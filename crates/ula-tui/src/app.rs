use crossterm::event::{Event, KeyEvent};
use tui_input::backend::crossterm::to_input_request;
use ula_protocol::interface::{UIReceive, UISend};
use ula_protocol::universal::{MessageEventType, UserInteraction};

use crate::modal::{Modal, ModalAction};

/// Something the main loop has to do outside of pure state mutation.
#[derive(Debug)]
pub enum Effect {
    Send(UISend),
    Flush(Turn),
}

/// A finished exchange, ready to be pushed into the terminal scrollback.
#[derive(Debug, Clone)]
pub enum Turn {
    User(String),
    Assistant {
        reasoning: Option<String>,
        content: String,
    },
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Quit,
    Submit,
    Interrupt,
    ScrollUp,
    ScrollDown,
    Edit(KeyEvent),
    Modal(KeyEvent),
}

#[derive(Debug, Default)]
pub struct Live {
    pub reasoning: String,
    pub content: String,
}

impl Live {
    fn clear(&mut self) {
        self.reasoning.clear();
        self.content.clear();
    }
}

#[derive(Debug)]
pub struct Scroll {
    /// First visible wrapped line, synced by the renderer.
    pub offset: u16,
    /// Follow the tail of the content instead of holding a position.
    pub follow: bool,
}

impl Default for Scroll {
    fn default() -> Self {
        Self {
            offset: 0,
            follow: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct App {
    pub input: tui_input::Input,
    pub live: Live,
    pub scroll: Scroll,
    pub error: Option<String>,
    pub busy: bool,
    pub modal: Option<Modal>,
    pub quit: bool,
}

impl App {
    pub fn on_ui(&mut self, event: UIReceive) -> Vec<Effect> {
        match event {
            UIReceive::Message { message } => match message.message_type {
                MessageEventType::Streaming => {
                    self.live.reasoning.push_str(&message.reasoning.unwrap_or_default());
                    self.live.content.push_str(&message.content);
                    self.busy = true;
                    vec![]
                }
                MessageEventType::Content => {
                    self.busy = false;
                    self.live.clear();
                    vec![Effect::Flush(Turn::Assistant {
                        reasoning: message.reasoning.filter(|text| !text.is_empty()),
                        content: message.content,
                    })]
                }
                // Reserved for session resume; no UI representation yet.
                MessageEventType::FullConversation => vec![],
            },
            UIReceive::Error { error } => {
                self.busy = false;
                self.error = Some(error);
                vec![]
            }
            UIReceive::Request {
                request_id,
                request_type,
                message,
            } => {
                self.modal = Some(Modal::Confirm {
                    request_id,
                    request_type,
                    message,
                });
                vec![]
            }
        }
    }

    pub fn on_action(&mut self, action: Action, page: u16) -> Vec<Effect> {
        match action {
            Action::None => vec![],
            Action::Quit => {
                self.quit = true;
                vec![]
            }
            Action::Submit => self.submit(),
            Action::Interrupt => vec![Effect::Send(UISend::UserInteraction {
                interaction: UserInteraction::Interrupt,
            })],
            Action::ScrollUp => {
                self.scroll.follow = false;
                self.scroll.offset = self.scroll.offset.saturating_sub(page);
                vec![]
            }
            Action::ScrollDown => {
                self.scroll.offset = self.scroll.offset.saturating_add(page);
                vec![]
            }
            Action::Edit(key) => {
                if let Some(request) = to_input_request(&Event::Key(key)) {
                    self.input.handle(request);
                }
                vec![]
            }
            Action::Modal(key) => self.answer_modal(key),
        }
    }

    fn submit(&mut self) -> Vec<Effect> {
        let text = self.input.value_and_reset();
        if text.trim().is_empty() {
            return vec![];
        }
        self.busy = true;
        self.error = None;
        self.live.clear();
        self.scroll = Scroll::default();
        vec![
            Effect::Flush(Turn::User(text.clone())),
            Effect::Send(UISend::UserInteraction {
                interaction: UserInteraction::Text { text },
            }),
        ]
    }

    fn answer_modal(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(mut modal) = self.modal.take() else {
            return vec![];
        };
        match modal.on_key(key) {
            Some(ModalAction::Confirm { allow, reason }) => {
                vec![Effect::Send(UISend::RequestResult {
                    request_id: modal.request_id(),
                    allow,
                    reason,
                })]
            }
            None => {
                self.modal = Some(modal);
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};
    use ula_protocol::interface::UIReceive;
    use ula_protocol::universal::{MessageEvent, MessageEventType};

    use super::*;

    fn edit(code: KeyCode) -> Action {
        Action::Edit(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn stream(reasoning: Option<&str>, content: &str) -> UIReceive {
        UIReceive::Message {
            message: MessageEvent {
                reasoning: reasoning.map(Into::into),
                content: content.into(),
                message_type: MessageEventType::Streaming,
            },
        }
    }

    fn turn(reasoning: Option<&str>, content: &str) -> UIReceive {
        UIReceive::Message {
            message: MessageEvent {
                reasoning: reasoning.map(Into::into),
                content: content.into(),
                message_type: MessageEventType::Content,
            },
        }
    }

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            app.on_action(edit(KeyCode::Char(character)), 1);
        }
    }

    #[test]
    fn stream_appends_deltas() {
        let mut app = App::default();
        app.on_ui(stream(Some("think"), "hel"));
        app.on_ui(stream(None, "lo"));
        assert_eq!(app.live.reasoning, "think");
        assert_eq!(app.live.content, "hello");
        assert!(app.busy);
    }

    #[test]
    fn content_flushes_the_turn() {
        let mut app = App::default();
        app.on_ui(stream(None, "hi"));
        let effects = app.on_ui(turn(None, "hi"));
        assert!(app.live.content.is_empty());
        assert!(!app.busy);
        assert!(matches!(
            effects.as_slice(),
            [Effect::Flush(Turn::Assistant { content, .. })] if content == "hi"
        ));
    }

    #[test]
    fn submit_flushes_user_turn_then_sends() {
        let mut app = App::default();
        type_text(&mut app, "yo");
        let effects = app.on_action(Action::Submit, 1);
        assert!(app.input.value().is_empty());
        assert!(app.busy);
        assert!(matches!(
            effects.as_slice(),
            [Effect::Flush(Turn::User(text)), Effect::Send(UISend::UserInteraction { interaction: UserInteraction::Text { text: sent } })]
                if text == "yo" && sent == "yo"
        ));
    }

    #[test]
    fn empty_submit_is_ignored() {
        let mut app = App::default();
        assert!(app.on_action(Action::Submit, 1).is_empty());
        assert!(!app.busy);
    }

    #[test]
    fn request_opens_a_modal_and_y_answers_it() {
        let mut app = App::default();
        app.on_ui(UIReceive::Request {
            request_id: 7,
            request_type: "bash".into(),
            message: "rm -rf /".into(),
        });
        assert!(app.modal.is_some());

        let effects = app.on_action(Action::Modal(KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
        )), 1);
        assert!(app.modal.is_none());
        assert!(matches!(
            effects.as_slice(),
            [Effect::Send(UISend::RequestResult { request_id: 7, allow: true, .. })]
        ));
    }

    #[test]
    fn modal_swallows_editing_keys() {
        let mut app = App::default();
        app.on_ui(UIReceive::Request {
            request_id: 1,
            request_type: "bash".into(),
            message: "ls".into(),
        });
        let effects = app.on_action(Action::Modal(KeyEvent::new(
            KeyCode::Char('z'),
            KeyModifiers::NONE,
        )), 1);
        assert!(effects.is_empty());
        assert!(app.modal.is_some());
    }

    #[test]
    fn scroll_up_leaves_follow_mode() {
        let mut app = App::default();
        assert!(app.scroll.follow);
        app.on_action(Action::ScrollUp, 5);
        assert!(!app.scroll.follow);
    }
}
