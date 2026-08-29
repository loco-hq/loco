use std::fmt;

/// Build-time errors from parsing schema YAML and validating templates.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    InvalidFieldType(String),
    MissingField(&'static str),
    TemplateVarNotDeclared {
        type_name: String,
        var: String,
    },
    TemplateVarNotCreateOnly {
        type_name: String,
        var: String,
    },
    TemplateVarNotSlug {
        type_name: String,
        var: String,
    },
    /// `type: object` (including as a list item) must declare `name:`
    /// (snake_case; codegen PascalCases it).
    ObjectMissingName {
        parent: String,
        field: String,
    },
    /// `kind:` is `document` (default) or `files`.
    UnknownKind {
        type_name: String,
        kind: String,
    },
    /// A `kind: files` instance is a directory of opaque bytes — the only
    /// properties it may declare are its pathTemplate variables.
    FilesTypeBodyProperty {
        type_name: String,
        property: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Yaml(e) => write!(f, "YAML parse error: {e}"),
            Error::InvalidFieldType(t) => write!(f, "invalid field type: {t}"),
            Error::MissingField(name) => write!(f, "missing required field: {name}"),
            Error::TemplateVarNotDeclared { type_name, var } => write!(
                f,
                "type '{type_name}' uses template variable '{var}' which must be declared as a property"
            ),
            Error::TemplateVarNotCreateOnly { type_name, var } => write!(
                f,
                "type '{type_name}' property '{var}' is used in pathTemplate and must have createOnly: true"
            ),
            Error::TemplateVarNotSlug { type_name, var } => write!(
                f,
                "type '{type_name}' property '{var}' is used in pathTemplate and must have type: slug"
            ),
            Error::ObjectMissingName { parent, field } => write!(
                f,
                "object type used by '{parent}.{field}' must declare name:"
            ),
            Error::UnknownKind { type_name, kind } => write!(
                f,
                "type '{type_name}' has unknown kind '{kind}' (expected 'document' or 'files')"
            ),
            Error::FilesTypeBodyProperty {
                type_name,
                property,
            } => write!(
                f,
                "type '{type_name}' is kind: files and may only declare pathTemplate variables, but declares '{property}'"
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
