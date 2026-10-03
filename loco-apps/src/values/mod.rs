//! Dataset-scoped secret and variable values.
//!
//! Declarations live on a version and are what `/schema` serves. A value
//! belongs to one dataset. Secrets go through [`SecretStore`]: the trait is
//! plaintext in, plaintext out, and encryption stays inside the impl so a
//! later cloud store can replace [`LakeSecretStore`] without a second
//! shape. Variables are plain lake records in [`VARIABLES`]; they have no
//! trait yet.
//!
//! Neither collection can be declared. A collection name is a slug
//! (`[a-z0-9_.-]+`), which excludes `$`, and `VersionSchema` refuses to
//! resolve a bare name that contains `$`. The lake key of a real collection
//! is `{owner}.{name}`, never the literal `$secrets` or `$variables`.
//!
//! The record id is the declaration reference a client puts in the path:
//! bare for this project, `{account}/{project}.{name}` for a dependency.
//! The lake stores that string as text, so `/` and `.` need no encoding.

mod lake;
mod seal;
mod variables;

use std::collections::HashMap;

use loco_lake::{DataAdapter, Error as LakeError, Record, Value};

pub use lake::LakeSecretStore;
pub use seal::parse_secret_key;
pub use seal::KeyStatus;
pub use variables::delete_variable;
pub use variables::get_variable;
pub use variables::list_variables;
pub use variables::put_variable;
pub use variables::VariableMeta;

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
/// `get` is for a later handler (#102); no route calls it.
pub trait SecretStore: Send + Sync {
    fn put(&self, dataset_id: &str, name: &str, plaintext: &str)
        -> Result<SecretMeta, SecretError>;
    fn get(&self, dataset_id: &str, name: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, dataset_id: &str, name: &str) -> Result<(), SecretError>;
    fn list(&self, dataset_id: &str) -> Result<Vec<SecretMeta>, SecretError>;
    /// Remove this dataset's secrets only. The HTTP dataset delete does not
    /// call this: those rows live in the lake, and
    /// [`DataAdapter::delete_dataset`] already removes every collection.
    /// A store that is not the lake implements this for real.
    fn delete_dataset(&self, dataset_id: &str) -> Result<(), SecretError>;
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
