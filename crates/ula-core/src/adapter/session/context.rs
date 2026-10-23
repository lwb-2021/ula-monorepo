use std::{collections::VecDeque, time::Duration};

use ula_protocol::core::EventToCore;

use crate::adapter::Provider;

const MAX_ATTEMPTS: u8 = 5;

#[derive(Default)]
pub(super) struct SessionContext {
    pub(super) provider_instance: Option<Provider>,
    pub(super) event_queue: VecDeque<EventToCore>,

    pub(super) retry: RetryContext,
}

pub(super) struct RetryContext {
    pub(super) attempts: u8,
}

impl Default for RetryContext {
    fn default() -> Self {
        Self {
            attempts: MAX_ATTEMPTS,
        }
    }
}

impl RetryContext {
    pub(super) fn reset_attempts(&mut self) {
        self.attempts = MAX_ATTEMPTS;
    }

    pub(super) fn retry(&mut self) -> Option<Duration> {
        let round = MAX_ATTEMPTS.checked_sub(self.attempts)?;
        if self.attempts == 0 {
            return None;
        }
        self.attempts -= 1;

        Some(Duration::from_millis(200) * 2u32.pow(u32::from(round)))
    }
}
