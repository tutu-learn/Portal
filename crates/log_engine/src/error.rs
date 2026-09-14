use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    #[error("query parse error: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),

    #[error("directory error: {0}")]
    Directory(#[from] tantivy::directory::error::OpenDirectoryError),

    #[error("data directory unavailable: {0}")]
    BadDirectory(PathBuf),

    #[error("log engine reader is temporarily unavailable during commit")]
    ReaderUnavailable,

    #[error("memory budget exceeded: {context} (rss {rss_mb} MiB / limit {limit_mb} MiB)")]
    MemoryBudgetExceeded {
        context: String,
        rss_mb: usize,
        limit_mb: usize,
    },
}

pub type LogResult<T> = std::result::Result<T, LogError>;
