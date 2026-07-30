use std::io::ErrorKind;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum APIError {
    #[error("{message}")]
    Retryable { message: String },

    #[error("Http Error: {code}")]
    Http { code: u16 },

    #[error(transparent)]
    Isahc { source: isahc::Error },

    #[error(transparent)]
    Io { source: std::io::Error },

    // Fatal
    #[error("Fatal Error - Not retrying: {message}")]
    Fatal { message: String },

    #[error("Fatal Error - Not retrying: {source}")]
    FatalIO { source: std::io::Error },

    #[error("Fatal Error - Not retrying: {source}")]
    FatalIsahc { source: isahc::Error },

    #[error("Fatal Error - Not retrying: Http Error: {code}")]
    FatalHttp { code: u16 },

    #[error("Fatal Error - Not retrying: Unsupported API Endpoint: {name}")]
    UnsupportedEndpoint { name: String },

    #[error("Fatal Error - Not retrying: Invalid API format: {message}")]
    InvalidAPIFormat { message: String },

    #[error("Fatal Error - Not retrying: Invalid JSON format: {source}")]
    JSON {
        #[from]
        source: serde_json::Error,
    },
}

impl APIError {
    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            Self::Retryable { .. } | Self::Http { .. } | Self::Isahc { .. } | Self::Io { .. }
        )
    }

    pub fn from_status_code(code: u16) -> Self {
        match code {
            429 => Self::Retryable {
                message: format!("Rate limited: HTTP {code}"),
            },
            0..400 => unreachable!(),
            400..500 => Self::FatalHttp { code },
            500..600 => Self::Http { code },
            code => Self::InvalidAPIFormat {
                message: format!("Invaild HTTP Response {code}"),
            },
        }
    }

    pub fn raise<T>(self) -> Result<T, Self> {
        Err(self)
    }
}

impl From<std::io::Error> for APIError {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::TimedOut
            | ErrorKind::NetworkDown
            | ErrorKind::NetworkUnreachable
            | ErrorKind::HostUnreachable
            | ErrorKind::Interrupted => Self::Io { source: value },
            _ => Self::FatalIO { source: value },
        }
    }
}

impl From<isahc::Error> for APIError {
    fn from(value: isahc::Error) -> Self {
        if value.is_network() {
            return Self::Isahc { source: value };
        }
        Self::FatalIsahc { source: value }
    }
}
