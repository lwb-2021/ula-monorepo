use std::fmt;

#[derive(Debug)]
pub enum Error {
    Lua { msg: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Lua { msg } => write!(f, "Lua: {msg}"),
        }
    }
}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Error::Lua { msg: e.to_string() }
    }
}

impl std::error::Error for Error {}

