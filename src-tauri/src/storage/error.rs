use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("{0}")]
    Other(String),
}

pub type StorageResult<T> = Result<T, StorageError>;
