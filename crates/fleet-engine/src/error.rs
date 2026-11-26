use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("git error: {0}")]
    Git(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("background task failed: {0}")]
    Join(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;
