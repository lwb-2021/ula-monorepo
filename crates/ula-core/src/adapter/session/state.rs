use crate::adapter::{APIError, APIEventStream};

#[derive(Default)]
pub enum SessionState {
    #[default]
    Idle,
    Halt,
    Error {
        error: APIError,
    },

    Streaming {
        stream: APIEventStream,
        partial: Partial,
        usage: Option<u32>,
    },

    WaitCompact,
    Compact {
        summary: String,
    },
}

#[derive(Debug, Default)]
pub struct Partial {
    pub(super) reasoning: Option<String>,
    pub(super) content: String,
}
