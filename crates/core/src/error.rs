use std::io;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("network error: {}", with_causes(.0))]
    Http(#[from] reqwest::Error),
    #[error("the server returned HTTP {0}")]
    Status(u16),
    #[error("the server sent a different part of the file than requested")]
    BadRange,
    #[error("the file changed on the server since the download started")]
    ResourceChanged,
    #[error("the connection closed before the download finished")]
    Incomplete,
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

/// reqwest's own message is often just "error sending request"; the reason
/// (refused, timed out, certificate problem) is in its sources.
fn with_causes(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !message.contains(&text) {
            message = format!("{message}: {text}");
        }
        source = cause.source();
    }
    message
}

impl Error {
    /// Whether trying the same request again might succeed.
    pub(crate) fn is_retryable(&self) -> bool {
        match self {
            Error::Http(e) => !e.is_builder() && !e.is_redirect(),
            Error::Status(code) => *code == 408 || *code == 429 || *code >= 500,
            Error::Incomplete => true,
            _ => false,
        }
    }

    /// The server is refusing extra connections or is temporarily overloaded.
    pub(crate) fn is_overloaded(&self) -> bool {
        matches!(self, Error::Status(429 | 503))
    }
}
