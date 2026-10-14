use std::fmt;

/// Daemon error. Every device failure carries the command that failed (spec 02 §2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Timeout(String),
    Nak(String),
    Disconnected,
    Io(String),
    Invalid(String),
    NotFound(String),
    Unsupported(String),
    /// The firmware profile is disabled on the keyboard (`showProfile` flag).
    ProfileDisabled(u8),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout(c) => write!(f, "device did not reply in time to {c}"),
            Error::Nak(c) => write!(f, "device rejected {c}"),
            Error::Disconnected => write!(f, "device disconnected"),
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Invalid(e) => write!(f, "{e}"),
            Error::NotFound(e) => write!(f, "not found: {e}"),
            Error::Unsupported(e) => write!(f, "not supported: {e}"),
            Error::ProfileDisabled(n) => write!(f, "profile {n} is disabled on the keyboard; enable it first"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<sherpa_core::ProtoError> for Error {
    fn from(e: sherpa_core::ProtoError) -> Self {
        Error::Invalid(e.0)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Invalid(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}
