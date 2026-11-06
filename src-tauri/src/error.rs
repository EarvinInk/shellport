use serde::{Serialize, Serializer};

/// Error type returned by every Tauri command. Wraps anyhow so internal code
/// can use `?` freely; serialized to the frontend as a plain message string.
#[derive(Debug)]
pub struct Error(pub anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Error(e.into())
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{:#}", self.0))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

pub type CmdResult<T> = Result<T, Error>;
