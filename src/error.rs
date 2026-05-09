use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("process error: {0}")]
    Process(String),
    #[error("api error (status {status}): {body}")]
    Api { status: u16, body: String },
    #[error("machine already started")]
    AlreadyStarted,
}
