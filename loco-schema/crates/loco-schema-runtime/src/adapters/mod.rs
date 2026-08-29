//! Pluggable persistence adapters for `InstanceStore<T>`.
//!
//! `SchemaPersistence<T>` decouples the cache (in `InstanceStore`) from the
//! on-disk format and storage backend. [`yaml_fs::YamlFsAdapter`] reads/writes
//! one YAML file per instance, matching the behavior the codebase had before
//! the adapter split.
//!
//! `kind: files` types are the other half: their instance is a directory of
//! opaque bytes rather than a document, persisted through
//! [`FileTreePersistence`] (filesystem implementation:
//! [`file_tree_fs::FileTreeFsAdapter`]).

pub mod file_tree_fs;
pub mod yaml_fs;

use crate::error::Error;
use crate::file_tree::{FileTree, FileTreeInstance};
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

/// Persists file-tree instances of `T` — a directory of opaque bytes at the
/// instance's key, with no `.yaml` suffix.
///
/// `FileTreeStore<T>` calls `list_trees` once at startup to learn which trees
/// exist, and delegates every read and write here; it never caches file bytes.
pub trait FileTreePersistence<T: FileTreeInstance>: Send + Sync {
    /// Every persisted tree, as `(key, identity)` pairs. Content is not read.
    /// Called once during `SchemaStore::load`.
    fn list_trees(&self) -> Result<Vec<(String, T)>, Error>;

    /// Read the whole tree at `key`. `None` when it does not exist.
    fn read_tree(&self, key: &str) -> Result<Option<FileTree>, Error>;

    /// Read one file out of the tree at `key`. `None` when either is absent.
    fn read_file(&self, key: &str, path: &str) -> Result<Option<Vec<u8>>, Error>;

    /// Replace the tree at `key` with `tree`. Whole-tree replace, atomic: a
    /// concurrent reader sees the old tree or the new one, never a mix.
    fn write_tree(&self, key: &str, tree: &FileTree) -> Result<(), Error>;

    /// When the tree at `key` was last replaced. `None` when it does not
    /// exist. Whole-tree replace is the only write, so this is the moment the
    /// current bytes arrived.
    fn modified_at(&self, key: &str) -> Result<Option<std::time::SystemTime>, Error>;

    /// Remove the tree at `key`. No-op if it does not exist.
    fn delete(&self, key: &str) -> Result<(), Error>;
}

pub use file_tree_fs::FileTreeFsAdapter;
pub use yaml_fs::YamlFsAdapter;
