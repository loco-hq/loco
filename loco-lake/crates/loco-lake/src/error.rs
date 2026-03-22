use std::fmt;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum Error {
    NotFound,
    AlreadyExists,
    InvalidTenant(String),
    Internal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "not found"),
            Error::AlreadyExists => write!(f, "already exists"),
            Error::InvalidTenant(msg) => write!(f, "invalid tenant: {msg}"),
            Error::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
