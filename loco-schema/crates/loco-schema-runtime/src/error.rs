use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    AlreadyExists(String),
    NotFound(String),
    MissingField(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Yaml(e) => write!(f, "YAML parse error: {e}"),
            Error::AlreadyExists(name) => write!(f, "already exists: {name}"),
            Error::NotFound(name) => write!(f, "not found: {name}"),
            Error::MissingField(name) => write!(f, "missing required field: {name}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(e: serde_yaml::Error) -> Self {
        Error::Yaml(e)
    }
}
