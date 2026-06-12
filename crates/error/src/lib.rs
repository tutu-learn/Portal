use thiserror::Error;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("sql translation error: {0}")]
    SqlTranslation(String),

    #[error("python error: {0}")]
    Python(String),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("queue error: {0}")]
    Queue(String),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;
