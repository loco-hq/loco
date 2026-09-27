use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    AlreadyExists(String),
    NotFound(String),
    MissingField(&'static str),
    /// A file-tree key or member path that is not a safe relative path
    /// (absolute, `..`, empty segment) or that traverses a symlink.
    InvalidPath(String),
    /// The new contents were renamed into place, but flushing the directory
    /// afterwards failed: readers see the new file now, but it may not
    /// survive a crash. The store treats the write as done (its cache
    /// follows the disk) and still returns this so the caller hears of it.
    NotDurable(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Yaml(e) => write!(f, "YAML parse error: {e}"),
            Error::AlreadyExists(name) => write!(f, "already exists: {name}"),
            Error::NotFound(name) => write!(f, "not found: {name}"),
            Error::MissingField(name) => write!(f, "missing required field: {name}"),
            Error::InvalidPath(path) => write!(f, "invalid path: {path}"),
            Error::NotDurable(e) => write!(f, "written but not flushed to disk: {e}"),
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
