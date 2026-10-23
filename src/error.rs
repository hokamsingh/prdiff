use thiserror::Error;

/// Exit codes: 0 ok, 2 bad input, 3 git failed, 4 conflicts (--fail-on-conflict), 5 description mismatch.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    BadInput(String),
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Conflicts(String),
    #[error("{0}")]
    Mismatch(String),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::BadInput(_) => 2,
            Error::Git(_) => 3,
            Error::Conflicts(_) => 4,
            Error::Mismatch(_) => 5,
        }
    }
}

pub fn bad(msg: impl Into<String>) -> anyhow::Error {
    Error::BadInput(msg.into()).into()
}

pub fn git_err(msg: impl Into<String>) -> anyhow::Error {
    Error::Git(msg.into()).into()
}
