use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Action;

/// Double Esc has to happen within this window to count as the interrupt leader sequence.
const LEADER_WINDOW: Duration = Duration::from_millis(800);

#[derive(Debug, Default)]
pub struct KeyRouter {
    leader: Option<Instant>,
}

impl KeyRouter {
    pub fn route(&mut self, key: KeyEvent, modal_open: bool) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.leader = None;
            return Action::Quit;
        }
        if modal_open {
            self.leader = None;
            return Action::Modal(key);
        }

        if key.code == KeyCode::Esc {
            if self.leader.is_some_and(|at| at.elapsed() <= LEADER_WINDOW) {
                self.leader = None;
                return Action::Interrupt;
            }
            self.leader = Some(Instant::now());
            return Action::None;
        }
        self.leader = None;

        match key.code {
            KeyCode::Enter => Action::Submit,
            KeyCode::PageUp | KeyCode::Up => Action::ScrollUp,
            KeyCode::PageDown | KeyCode::Down => Action::ScrollDown,
            _ => Action::Edit(key),
        }
    }
}
