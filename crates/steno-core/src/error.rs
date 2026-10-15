use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("failed to read {path}: {source}")]
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to write {path}: {source}")]
    FileWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("session '{0}' not found")]
    SessionNotFound(String),

    #[error("task '{0}' not found")]
    TaskNotFound(String),

    #[error("task '{0}' already exists")]
    TaskExists(String),

    #[error("no active task")]
    NoActiveTask,

    #[error("invalid spool event: {0}")]
    InvalidSpoolEvent(String),

    #[error("IPC error: {0}")]
    Ipc(String),

    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
