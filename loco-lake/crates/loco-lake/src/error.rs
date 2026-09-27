use std::fmt;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum Error {
    NotFound,
    AlreadyExists,
    InvalidDataset(String),
    /// A `LakeQuery` the lake cannot run as given (see `query.rs`).
    InvalidQuery(String),
    Internal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "not found"),
            Error::AlreadyExists => write!(f, "already exists"),
            Error::InvalidDataset(msg) => write!(f, "invalid dataset: {msg}"),
            Error::InvalidQuery(msg) => write!(f, "invalid query: {msg}"),
            Error::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
