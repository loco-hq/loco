//! File-tree instances: metadata that is a directory of files, not a YAML
//! document.
//!
//! A `kind: files` type has no body properties — its identity is entirely the
//! `pathTemplate`, and its value is an opaque tree of bytes stored at that path
//! (no `.yaml` suffix). [`FileTreeStore`] is the file-tree counterpart of
//! [`crate::InstanceStore`]: it caches which keys exist and delegates all I/O
//! to a [`crate::FileTreePersistence`] adapter.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use crate::adapters::FileTreePersistence;
use crate::error::Error;

/// Implemented by every generated `kind: files` type. Carries only the
/// template variables — the tree's bytes live in the adapter, not the struct.
pub trait FileTreeInstance: Clone + Send + Sync + Sized + 'static {
    /// This instance's persistence key (its namespace / relative path, with no
    /// extension).
    fn to_path(&self) -> String;

    /// Match a key against this type's `pathTemplate`. Returns the extracted
    /// template variables on success, or `None` if the key belongs to a
    /// different type.
    fn from_path(path: &str) -> Option<HashMap<String, String>>;

    /// Build the identity struct from template variables extracted by
    /// [`FileTreeInstance::from_path`].
    fn from_vars(vars: &HashMap<String, String>) -> Self;
}

/// An in-memory file tree: relative path → bytes.
///
/// Paths are validated on insert (see [`validate_relative_path`]), so a tree
/// can never carry `..`, an absolute path, or an empty segment out to disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTree {
    files: BTreeMap<String, Vec<u8>>,
}

impl FileTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file. Returns [`Error::InvalidPath`] if `path` is not a safe
    /// relative path.
    pub fn insert(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), Error> {
        validate_relative_path(path)?;
        self.files.insert(path.to_string(), bytes);
        Ok(())
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// Relative paths in sorted order.
    pub fn paths(&self) -> Vec<&str> {
        self.files.keys().map(|k| k.as_str()).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Total bytes across every file in the tree.
    pub fn total_bytes(&self) -> usize {
        self.files.values().map(|v| v.len()).sum()
    }
}

/// Reject anything that is not a plain relative path inside the tree:
/// absolute paths, `.` / `..` / empty segments, backslashes, and NUL.
pub fn validate_relative_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err(Error::InvalidPath(path.to_string()));
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(Error::InvalidPath(path.to_string()));
        }
    }
    Ok(())
}

/// Per-type cache of which file trees exist, backed by a
/// [`FileTreePersistence`] adapter.
///
/// The cache holds identities (template variables), never file bytes: reads go
/// to the adapter every time. Writes are whole-tree replace — there is no patch
/// of one file.
pub struct FileTreeStore<T: FileTreeInstance> {
    cache: RwLock<BTreeMap<String, Arc<T>>>,
    adapter: Arc<dyn FileTreePersistence<T>>,
}

impl<T: FileTreeInstance> FileTreeStore<T> {
    pub fn new(adapter: Arc<dyn FileTreePersistence<T>>) -> Self {
        Self {
            cache: RwLock::new(BTreeMap::new()),
            adapter,
        }
    }

    /// Record a tree that already exists on disk. Used by `SchemaStore::load`.
    pub fn insert_loaded(&self, key: String, instance: Arc<T>) {
        self.cache.write().unwrap().insert(key, instance);
    }

    pub fn get(&self, key: &str) -> Option<Arc<T>> {
        self.cache.read().unwrap().get(key).cloned()
    }

    pub fn has(&self, key: &str) -> bool {
        self.cache.read().unwrap().contains_key(key)
    }

    /// Every tree whose key starts with `prefix`.
    pub fn list(&self, prefix: &str) -> Vec<(String, Arc<T>)> {
        let cache = self.cache.read().unwrap();
        cache
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn list_all(&self) -> Vec<(String, Arc<T>)> {
        let cache = self.cache.read().unwrap();
        cache.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Read one file out of a tree. `None` when the tree or the file is absent.
    pub fn read_file(&self, key: &str, path: &str) -> Result<Option<Vec<u8>>, Error> {
        self.adapter.read_file(key, path)
    }

    /// Read a whole tree. `None` when the tree does not exist.
    pub fn read_tree(&self, key: &str) -> Result<Option<FileTree>, Error> {
        self.adapter.read_tree(key)
    }

    /// The files in a tree, sorted. `None` when the tree does not exist.
    pub fn list_files(&self, key: &str) -> Result<Option<Vec<String>>, Error> {
        Ok(self
            .read_tree(key)?
            .map(|t| t.paths().into_iter().map(|s| s.to_string()).collect()))
    }

    /// Replace the tree at `key` with `tree`, atomically. Creates it if absent.
    pub fn put(&self, key: &str, tree: &FileTree) -> Result<Arc<T>, Error> {
        let vars = T::from_path(key).ok_or_else(|| Error::InvalidPath(key.to_string()))?;
        self.adapter.write_tree(key, tree)?;
        let arc = Arc::new(T::from_vars(&vars));
        self.cache
            .write()
            .unwrap()
            .insert(key.to_string(), arc.clone());
        Ok(arc)
    }

    pub fn delete(&self, key: &str) -> Result<(), Error> {
        {
            let cache = self.cache.read().unwrap();
            if !cache.contains_key(key) {
                return Err(Error::NotFound(key.to_string()));
            }
        }
        self.adapter.delete(key)?;
        self.cache.write().unwrap().remove(key);
        Ok(())
    }

    /// Drop every tree whose key starts with `prefix`. Lets a project or
    /// version delete cascade into file-tree instances.
    pub fn delete_by_prefix(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let to_delete: Vec<String> = {
            let cache = self.cache.read().unwrap();
            cache
                .range(prefix.to_string()..)
                .take_while(|(k, _)| k.starts_with(prefix))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for key in &to_delete {
            let _ = self.adapter.delete(key);
        }
        let mut cache = self.cache.write().unwrap();
        for key in &to_delete {
            cache.remove(key);
        }
        Ok(to_delete)
    }

    /// Duplicate every tree under `from_prefix` to the same suffix under
    /// `to_prefix`. The copy primitive a later copy-version builds on; keys
    /// that do not match this type's template after rewriting are skipped.
    pub fn copy_by_prefix(&self, from_prefix: &str, to_prefix: &str) -> Result<Vec<String>, Error> {
        let sources: Vec<String> = {
            let cache = self.cache.read().unwrap();
            cache
                .range(from_prefix.to_string()..)
                .take_while(|(k, _)| k.starts_with(from_prefix))
                .map(|(k, _)| k.clone())
                .collect()
        };
        let mut copied = Vec::new();
        for src in &sources {
            let dest = format!("{to_prefix}{}", &src[from_prefix.len()..]);
            let Some(vars) = T::from_path(&dest) else {
                continue;
            };
            let Some(tree) = self.adapter.read_tree(src)? else {
                continue;
            };
            self.adapter.write_tree(&dest, &tree)?;
            self.cache
                .write()
                .unwrap()
                .insert(dest.clone(), Arc::new(T::from_vars(&vars)));
            copied.push(dest);
        }
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_relative_paths() {
        for bad in [
            "",
            "/etc/passwd",
            "../escape",
            "assets/../../escape",
            "assets/./x",
            "assets//x",
            "assets/",
            "assets\\x",
        ] {
            assert!(
                validate_relative_path(bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn accepts_ordinary_relative_paths() {
        for ok in ["index.html", "assets/index-abc123.js", "a/b/c/d.png"] {
            assert!(
                validate_relative_path(ok).is_ok(),
                "expected {ok:?} to pass"
            );
        }
    }

    #[test]
    fn insert_rejects_unsafe_paths() {
        let mut tree = FileTree::new();
        assert!(matches!(
            tree.insert("../x", b"hi".to_vec()),
            Err(Error::InvalidPath(_))
        ));
        assert!(tree.is_empty());

        tree.insert("index.html", b"hi".to_vec()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.total_bytes(), 2);
        assert_eq!(tree.get("index.html"), Some(b"hi".as_slice()));
        assert_eq!(tree.paths(), vec!["index.html"]);
    }
}
