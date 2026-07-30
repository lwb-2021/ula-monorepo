use crossterm::event::{KeyCode, KeyEvent};

/// A modal steals every key until it is answered or dismissed.
#[derive(Debug, Clone)]
pub enum Modal {
    Confirm {
        request_id: u8,
        request_type: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalAction {
    Confirm { allow: bool, reason: Option<String> },
}

impl Modal {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Confirm { .. } => " 权限请求 ",
        }
    }

    pub fn request_id(&self) -> u8 {
        match self {
            Self::Confirm { request_id, .. } => *request_id,
        }
    }

    /// `None` keeps the modal open.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<ModalAction> {
        match key.code {
            KeyCode::Char('y' | 'Y') => Some(ModalAction::Confirm {
                allow: true,
                reason: None,
            }),
            KeyCode::Char('n' | 'N') | KeyCode::Enter => Some(ModalAction::Confirm {
                allow: false,
                reason: None,
            }),
            KeyCode::Esc => Some(ModalAction::Confirm {
                allow: false,
                reason: Some("dismissed".into()),
            }),
            _ => None,
        }
    }
}
