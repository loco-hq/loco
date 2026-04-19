use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    InvalidFieldType(String),
    MissingField(&'static str),
    InvalidValue(String),
    AlreadyExists(String),
    NotFound(String),
    TemplateVarNotDeclared { type_name: String, var: String },
    TemplateVarNotCreateOnly { type_name: String, var: String },
    TemplateVarNotString { type_name: String, var: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Yaml(e) => write!(f, "YAML parse error: {e}"),
            Error::InvalidFieldType(t) => write!(f, "invalid field type: {t}"),
            Error::MissingField(name) => write!(f, "missing required field: {name}"),
            Error::InvalidValue(msg) => write!(f, "invalid value: {msg}"),
            Error::AlreadyExists(name) => write!(f, "already exists: {name}"),
            Error::NotFound(name) => write!(f, "not found: {name}"),
            Error::TemplateVarNotDeclared { type_name, var } => write!(
                f,
                "type '{type_name}' uses template variable '{var}' which must be declared as a property"
            ),
            Error::TemplateVarNotCreateOnly { type_name, var } => write!(
                f,
                "type '{type_name}' property '{var}' is used in filePathTemplate and must have createOnly: true"
            ),
            Error::TemplateVarNotString { type_name, var } => write!(
                f,
                "type '{type_name}' property '{var}' is used in filePathTemplate and must have type: string"
            ),
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
