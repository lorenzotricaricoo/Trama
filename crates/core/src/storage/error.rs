//! Errors raised by the storage layer.

use std::path::PathBuf;

/// Result type of the storage layer.
pub type Result<T, E = StorageError> = std::result::Result<T, E>;

/// What can go wrong while opening or using a library.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Another process already owns the library (principle A2).
    #[error("the library at {0} is already open in another process")]
    Locked(PathBuf),

    /// The library was written by a newer version of Trama.
    #[error("the library uses schema version {found}, newer than the supported {supported}")]
    NewerSchema { found: u32, supported: u32 },

    /// No record with this id, or it is in the trash when the operation needs it not to be.
    #[error("{kind} {id} not found")]
    NotFound { kind: &'static str, id: String },

    /// The operation does not apply to this record in its current state.
    #[error("invalid operation: {0}")]
    Invalid(&'static str),

    /// A stored value does not match the schema (a bug or a damaged database).
    #[error("corrupt value in the library: {0}")]
    Corrupt(String),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StorageError {
    pub(crate) fn not_found(kind: &'static str, id: impl ToString) -> Self {
        Self::NotFound {
            kind,
            id: id.to_string(),
        }
    }
}
