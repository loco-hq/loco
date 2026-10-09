//! Dataset-scoped secret and variable values.
//!
//! Declarations live on a version and are what `/schema` serves. A value
//! belongs to one dataset. Both stores are plaintext in, plaintext out.
//! Encryption stays inside [`LakeSecretStore`], so a later cloud store can
//! replace it without a second shape. [`VariableStore`] is the same idea
//! for a plain string: [`LakeVariableStore`] writes [`VARIABLES`] and needs
//! no key.
//!
//! Neither collection can be declared. A collection name is a slug
//! (`[a-z0-9_.-]+`), which excludes `$`, and `VersionSchema` refuses to
//! resolve a bare name that contains `$`. The lake key of a real collection
//! is `{owner}.{name}`, never the literal `$secrets` or `$variables`.
//!
//! The record id of a loose declaration is the canonical reference: bare when
//! it belongs to this project, `{account}/{project}.{name}` for a dependency.
//! A qualified name for this project is stored as the bare name. A connection
//! value is `{qualified integration}:{name}` (`sf_east:token`,
//! `alice/pkg.store:consumer_key`). The lake stores that string as text, so
//! `/`, `.`, and `:` need no encoding.

mod lake;
mod seal;
mod variables;

use std::collections::HashMap;

use loco_lake::{DataAdapter, Error as LakeError, Record, Value};

pub use lake::LakeSecretStore;
pub use seal::parse_secret_key;
pub use seal::KeyStatus;
pub use variables::with_default;
pub use variables::LakeVariableStore;

/// Lake collection for secret ciphertext. Not a schema collection.
pub const SECRETS: &str = "$secrets";
/// Lake collection for variable values. Not a schema collection.
pub const VARIABLES: &str = "$variables";

/// Stamped on secret and variable rows. They are not written through `/data`,
/// so they are not attributed to a person.
const ACTOR: &str = "system";

/// `name` and `updated_at` of one stored secret. No plaintext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMeta {
    pub name: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub enum SecretError {
    /// `LOCO_SECRET_KEY` is missing or malformed. The string names it.
    Unavailable(String),
    /// The name cannot be a record id.
    InvalidName(String),
    NotFound,
    /// Encrypt or decrypt failed. One error for a wrong key and for garbage.
    Failed(String),
    Lake(LakeError),
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(msg) | Self::InvalidName(msg) | Self::Failed(msg) => {
                write!(f, "{msg}")
            }
            Self::NotFound => write!(f, "secret value not found"),
            Self::Lake(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SecretError {}

impl From<LakeError> for SecretError {
    fn from(err: LakeError) -> Self {
        match err {
            LakeError::NotFound => Self::NotFound,
            other => Self::Lake(other),
        }
    }
}

/// Plaintext in, plaintext out. Encryption is the impl's concern.
/// `dataset_id` is `{account}/{project}/{dataset}`, the lake's dataset id.
/// `get` has no route. Action handlers call it for their own package's
/// declarations, on the request's dataset.
pub trait SecretStore: Send + Sync {
    fn put(&self, dataset_id: &str, name: &str, plaintext: &str)
        -> Result<SecretMeta, SecretError>;
    fn get(&self, dataset_id: &str, name: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, dataset_id: &str, name: &str) -> Result<(), SecretError>;
    fn list(&self, dataset_id: &str) -> Result<Vec<SecretMeta>, SecretError>;
    /// Remove this dataset's secrets. Dataset and project delete call this
    /// first, then [`VariableStore::delete_dataset`], then
    /// [`crate::source::LakeSource::purge_dataset`]. A failure here leaves
    /// the variable rows and the lake in place. The lake impl deletes
    /// `$secrets` rows the purge would remove anyway.
    fn delete_dataset(&self, dataset_id: &str) -> Result<(), SecretError>;
}

/// `name`, `value`, and `updated_at` of one stored variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableMeta {
    pub name: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub enum VariableError {
    /// The name cannot be a record id.
    InvalidName(String),
    NotFound,
    Lake(LakeError),
}

impl std::fmt::Display for VariableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(msg) => write!(f, "{msg}"),
            Self::NotFound => write!(f, "variable value not found"),
            Self::Lake(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for VariableError {}

impl From<LakeError> for VariableError {
    fn from(err: LakeError) -> Self {
        match err {
            LakeError::NotFound => Self::NotFound,
            other => Self::Lake(other),
        }
    }
}

/// Plaintext in, plaintext out. A variable needs no key.
///
/// `dataset_id` is `{account}/{project}/{dataset}`, the lake's dataset id.
/// [`Self::get`] returns the stored string, including `""`, or `None` when
/// no row exists. A declaration default is not this store's concern:
/// callers apply it with [`with_default`].
pub trait VariableStore: Send + Sync {
    fn set(&self, dataset_id: &str, name: &str, value: &str)
        -> Result<VariableMeta, VariableError>;
    fn get(&self, dataset_id: &str, name: &str) -> Result<Option<String>, VariableError>;
    fn delete(&self, dataset_id: &str, name: &str) -> Result<(), VariableError>;
    fn list(&self, dataset_id: &str) -> Result<Vec<VariableMeta>, VariableError>;
    /// Remove this dataset's variables. Dataset and project delete call this
    /// after [`SecretStore::delete_dataset`] and before
    /// [`crate::source::LakeSource::purge_dataset`]. A failure here leaves
    /// the lake in place. The lake impl deletes `$variables` rows the purge
    /// would remove anyway.
    fn delete_dataset(&self, dataset_id: &str) -> Result<(), VariableError>;
}

pub(crate) fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('\0') {
        return Err(format!("invalid config value name: {name:?}"));
    }
    Ok(())
}

fn meta_from(record: &Record) -> SecretMeta {
    SecretMeta {
        name: record.id.clone(),
        updated_at: record.updated_at.clone(),
    }
}

fn upsert(
    data: &dyn DataAdapter,
    dataset_id: &str,
    collection: &str,
    name: &str,
    fields: HashMap<String, Value>,
) -> Result<Record, LakeError> {
    data.upsert(dataset_id, collection, name, ACTOR, fields)
}
