use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("not configured: set base URL and API key")]
    NotConfigured,
    #[error("http {status}: {message}")]
    Api { status: u16, message: String, code: Option<String> },
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type ClientResult<T> = Result<T, ClientError>;
