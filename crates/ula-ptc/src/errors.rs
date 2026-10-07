use thiserror::Error;

#[derive(Debug, Error)]
pub enum PTCError {
    #[error("{msg}")]
    Lua { msg: String },

    #[error("Internal state error:{msg}")]
    Internal { msg: String },
}

impl PTCError {
    pub fn raise<T>(self) -> Result<T, PTCError> {
        Err(self)
    }
}

impl From<mlua::Error> for PTCError {
    fn from(value: mlua::Error) -> Self {
        Self::Lua {
            msg: value.to_string(),
        }
    }
}
