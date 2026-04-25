//! Pluggable persistence adapters for `InstanceStore<T>`.
//!
//! `SchemaPersistence<T>` decouples the cache (in `InstanceStore`) from the
//! on-disk format and storage backend. Today only [`yaml_fs::YamlFsAdapter`] is
//! provided — it reads/writes YAML files under a directory tree, matching the
//! behavior the codebase had before the adapter split.

pub mod yaml_fs;

use crate::error::Error;
use crate::store::SchemaInstance;

/// Persists instances of `T` somewhere — filesystem, database, network store.
///
/// `InstanceStore<T>` calls `write` and `delete` on every mutation, and calls
/// `load_all` once at startup to populate its in-memory cache.
pub trait SchemaPersistence<T: SchemaInstance>: Send + Sync {
    /// Read every persisted instance and return `(key, value)` pairs.
    /// Called once during `SchemaStore::load`.
    fn load_all(&self) -> Result<Vec<(String, T)>, Error>;

    /// Persist `value` under `key`. Overwrites any existing entry.
    fn write(&self, key: &str, value: &T) -> Result<(), Error>;

    /// Remove the entry at `key`. No-op if it does not exist.
    fn delete(&self, key: &str) -> Result<(), Error>;
}

pub use yaml_fs::YamlFsAdapter;
