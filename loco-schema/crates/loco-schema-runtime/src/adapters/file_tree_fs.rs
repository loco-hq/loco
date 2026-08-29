//! Filesystem-backed adapter for `kind: files` instances.
//!
//! A key maps to a *directory* under `dir` — the pathTemplate prefix with no
//! `.yaml` suffix — and the files inside it are opaque bytes. Nothing outside
//! that directory is ever read or written: keys and member paths are validated
//! (no `..`, no absolute paths), and symlinks are never followed.
//!
//! Writes are whole-tree replace and atomic: the new tree is staged in a
//! sibling temp directory and swapped in with `rename`, so a reader sees either
//! the old tree or the new one, never a half-written mix.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::adapters::FileTreePersistence;
use crate::error::Error;
use crate::file_tree::{validate_relative_path, FileTree, FileTreeInstance};

pub struct FileTreeFsAdapter<T: FileTreeInstance> {
    dir: PathBuf,
    _marker: PhantomData<fn() -> T>,
}

impl<T: FileTreeInstance> FileTreeFsAdapter<T> {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            _marker: PhantomData,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory a key lives at. Keys are relative paths, same rules as
    /// files inside a tree.
    fn resolve(&self, key: &str) -> Result<PathBuf, Error> {
        validate_relative_path(key)?;
        Ok(self.dir.join(key))
    }

    /// `Ok(None)` when the tree does not exist. A symlinked tree root is
    /// refused rather than followed.
    fn open_tree(&self, key: &str) -> Result<Option<PathBuf>, Error> {
        let root = self.resolve(key)?;
        if is_symlink(&root) {
            return Err(Error::InvalidPath(key.to_string()));
        }
        match std::fs::metadata(&root) {
            Ok(md) if md.is_dir() => Ok(Some(root)),
            _ => Ok(None),
        }
    }
}

impl<T: FileTreeInstance> FileTreePersistence<T> for FileTreeFsAdapter<T> {
    fn list_trees(&self) -> Result<Vec<(String, T)>, Error> {
        let mut out = Vec::new();
        collect_trees::<T>(&self.dir, &self.dir, &mut out)?;
        Ok(out)
    }

    fn read_tree(&self, key: &str) -> Result<Option<FileTree>, Error> {
        let Some(root) = self.open_tree(key)? else {
            return Ok(None);
        };
        let mut tree = FileTree::new();
        collect_files(&root, "", &mut tree)?;
        Ok(Some(tree))
    }

    fn read_file(&self, key: &str, path: &str) -> Result<Option<Vec<u8>>, Error> {
        let Some(root) = self.open_tree(key)? else {
            return Ok(None);
        };
        let file = resolve_in_tree(&root, path)?;
        match std::fs::metadata(&file) {
            Ok(md) if md.is_file() => Ok(Some(std::fs::read(&file)?)),
            _ => Ok(None),
        }
    }

    fn write_tree(&self, key: &str, tree: &FileTree) -> Result<(), Error> {
        let target = self.resolve(key)?;
        let parent = target
            .parent()
            .ok_or_else(|| Error::InvalidPath(key.to_string()))?
            .to_path_buf();
        std::fs::create_dir_all(&parent)?;

        let staging = temp_sibling(&parent, "staging");
        if let Err(e) = stage_tree(&staging, tree) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }

        if is_symlink(&target) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(Error::InvalidPath(key.to_string()));
        }

        // rename(2) will not replace a non-empty directory, so swap through a
        // second name and only then drop the old tree.
        if target.exists() {
            let retired = temp_sibling(&parent, "retired");
            std::fs::rename(&target, &retired)?;
            match std::fs::rename(&staging, &target) {
                Ok(()) => {
                    let _ = std::fs::remove_dir_all(&retired);
                }
                Err(e) => {
                    let _ = std::fs::rename(&retired, &target);
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(e.into());
                }
            }
        } else if let Err(e) = std::fs::rename(&staging, &target) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.into());
        }
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        let root = self.resolve(key)?;
        if is_symlink(&root) {
            return Err(Error::InvalidPath(key.to_string()));
        }
        if root.is_dir() {
            std::fs::remove_dir_all(&root)?;
        }
        prune_empty_parents(&self.dir, root.parent());
        Ok(())
    }
}

/// Stage a whole tree under `staging`, which must not exist yet.
fn stage_tree(staging: &Path, tree: &FileTree) -> Result<(), Error> {
    std::fs::create_dir_all(staging)?;
    for (rel, bytes) in tree.iter() {
        // Paths were validated on insert; re-check so a hand-built tree cannot
        // sneak a traversal past staging.
        let path = resolve_in_tree(staging, rel)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    Ok(())
}

/// Resolve `rel` inside a tree root, refusing to traverse a symlink at any
/// component. Components that do not exist yet are fine — the caller's read or
/// write reports that.
fn resolve_in_tree(root: &Path, rel: &str) -> Result<PathBuf, Error> {
    validate_relative_path(rel)?;
    let mut path = root.to_path_buf();
    for seg in rel.split('/') {
        path.push(seg);
        if is_symlink(&path) {
            return Err(Error::InvalidPath(rel.to_string()));
        }
    }
    Ok(path)
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|md| md.file_type().is_symlink())
        .unwrap_or(false)
}

/// Walk `current`, recording every directory whose key matches `T`'s template.
/// A matched directory is a tree root and is not descended into.
fn collect_trees<T: FileTreeInstance>(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, T)>,
) -> Result<(), Error> {
    if !current.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        match T::from_path(&rel) {
            Some(vars) => out.push((rel, T::from_vars(&vars))),
            None => collect_trees::<T>(root, &path, out)?,
        }
    }
    Ok(())
}

/// Read every regular file under `dir` into `tree`. Symlinks are skipped, not
/// followed — a stray one on disk must not pull bytes in from outside.
fn collect_files(dir: &Path, prefix: &str, tree: &mut FileTree) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if file_type.is_dir() {
            collect_files(&entry.path(), &rel, tree)?;
        } else if file_type.is_file() {
            tree.insert(&rel, std::fs::read(entry.path())?)?;
        }
    }
    Ok(())
}

/// Remove now-empty directories up to (but not including) `root`.
fn prune_empty_parents(root: &Path, from: Option<&Path>) {
    let mut dir = from;
    while let Some(d) = dir {
        if d == root {
            break;
        }
        match std::fs::remove_dir(d) {
            Ok(()) => dir = d.parent(),
            Err(_) => break,
        }
    }
}

/// A unique sibling name in `parent`, used to stage and retire trees during an
/// atomic swap. Leading `.` plus the non-template shape keeps it invisible to
/// `list_trees`.
fn temp_sibling(parent: &Path, tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".loco-{tag}-{}-{nanos}-{n}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_tree::FileTreeStore;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq)]
    struct TestBundle {
        project: String,
        version: String,
    }

    impl FileTreeInstance for TestBundle {
        fn to_path(&self) -> String {
            format!("{}/versions/{}/bundle", self.project, self.version)
        }
        fn from_path(path: &str) -> Option<HashMap<String, String>> {
            let segs: Vec<&str> = path.split('/').collect();
            if segs.len() != 5 || segs[2] != "versions" || segs[4] != "bundle" {
                return None;
            }
            let mut vars = HashMap::new();
            vars.insert("project".to_string(), format!("{}/{}", segs[0], segs[1]));
            vars.insert("version".to_string(), segs[3].to_string());
            Some(vars)
        }
        fn from_vars(vars: &HashMap<String, String>) -> Self {
            TestBundle {
                project: vars.get("project").cloned().unwrap_or_default(),
                version: vars.get("version").cloned().unwrap_or_default(),
            }
        }
    }

    const KEY: &str = "ben/blog/versions/0.0.1-dev/bundle";

    fn adapter(dir: &Path) -> FileTreeFsAdapter<TestBundle> {
        FileTreeFsAdapter::new(dir.to_path_buf())
    }

    fn tree(files: &[(&str, &str)]) -> FileTree {
        let mut t = FileTree::new();
        for (path, body) in files {
            t.insert(path, body.as_bytes().to_vec()).unwrap();
        }
        t
    }

    fn store(dir: &Path) -> FileTreeStore<TestBundle> {
        let adapter: Arc<dyn FileTreePersistence<TestBundle>> = Arc::new(adapter(dir));
        let store = FileTreeStore::new(adapter.clone());
        for (key, inst) in adapter.list_trees().unwrap() {
            store.insert_loaded(key, Arc::new(inst));
        }
        store
    }

    #[test]
    fn write_then_read_tree_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());

        a.write_tree(
            KEY,
            &tree(&[
                ("index.html", "<h1>hi</h1>"),
                ("assets/app.js", "console.log(1)"),
            ]),
        )
        .unwrap();

        // Stored as a directory at the key, with no `.yaml` anywhere.
        let root = dir.path().join(KEY);
        assert!(root.is_dir());
        assert!(root.join("index.html").is_file());
        assert!(root.join("assets/app.js").is_file());
        assert!(!dir.path().join(format!("{KEY}.yaml")).exists());

        let loaded = a.read_tree(KEY).unwrap().unwrap();
        assert_eq!(loaded.paths(), vec!["assets/app.js", "index.html"]);
        assert_eq!(loaded.get("index.html"), Some("<h1>hi</h1>".as_bytes()));

        assert_eq!(
            a.read_file(KEY, "assets/app.js").unwrap(),
            Some(b"console.log(1)".to_vec())
        );
        assert_eq!(a.read_file(KEY, "missing.css").unwrap(), None);
    }

    #[test]
    fn missing_tree_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());
        assert!(a.read_tree(KEY).unwrap().is_none());
        assert!(a.read_file(KEY, "index.html").unwrap().is_none());
        // Nothing on disk at all — boot must not fail.
        assert!(a.list_trees().unwrap().is_empty());
    }

    #[test]
    fn write_replaces_the_whole_tree() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());

        a.write_tree(
            KEY,
            &tree(&[("index.html", "v1"), ("assets/old.js", "old")]),
        )
        .unwrap();
        a.write_tree(
            KEY,
            &tree(&[("index.html", "v2"), ("assets/new.js", "new")]),
        )
        .unwrap();

        let loaded = a.read_tree(KEY).unwrap().unwrap();
        assert_eq!(loaded.paths(), vec!["assets/new.js", "index.html"]);
        assert_eq!(loaded.get("index.html"), Some(b"v2".as_slice()));
        // The replaced file is gone from disk, not just from the tree listing.
        assert!(!dir.path().join(KEY).join("assets/old.js").exists());
        // No staging or retired directories left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("ben/blog/versions/0.0.1-dev"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["bundle".to_string()]);
    }

    #[test]
    fn a_failed_write_leaves_the_previous_tree_intact() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());
        a.write_tree(KEY, &tree(&[("index.html", "v1")])).unwrap();

        // A file and a directory cannot share a name — staging fails partway.
        let mut bad = FileTree::new();
        bad.insert("a", b"file".to_vec()).unwrap();
        bad.insert("a/b", b"under a file".to_vec()).unwrap();
        assert!(a.write_tree(KEY, &bad).is_err());

        let loaded = a.read_tree(KEY).unwrap().unwrap();
        assert_eq!(loaded.get("index.html"), Some(b"v1".as_slice()));
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("ben/blog/versions/0.0.1-dev"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["bundle".to_string()]);
    }

    #[test]
    fn delete_removes_the_tree_and_prunes_parents() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());
        a.write_tree(KEY, &tree(&[("index.html", "hi")])).unwrap();

        a.delete(KEY).unwrap();
        assert!(!dir.path().join(KEY).exists());
        assert!(!dir.path().join("ben/blog").exists());
        assert!(dir.path().exists());

        // Deleting what is not there is a no-op.
        a.delete(KEY).unwrap();
    }

    #[test]
    fn traversal_and_symlinks_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());
        a.write_tree(KEY, &tree(&[("index.html", "hi")])).unwrap();

        assert!(matches!(
            a.read_file(KEY, "../../../../etc/passwd"),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            a.read_file(KEY, "/etc/passwd"),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            a.write_tree("../escape/bundle", &FileTree::new()),
            Err(Error::InvalidPath(_))
        ));

        // A symlink planted inside the tree is neither followed nor collected.
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, "s3cret").unwrap();
        std::os::unix::fs::symlink(&secret, dir.path().join(KEY).join("link.txt")).unwrap();
        assert!(matches!(
            a.read_file(KEY, "link.txt"),
            Err(Error::InvalidPath(_))
        ));
        let loaded = a.read_tree(KEY).unwrap().unwrap();
        assert_eq!(loaded.paths(), vec!["index.html"]);
    }

    #[test]
    fn list_trees_finds_keys_matching_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let a = adapter(dir.path());
        a.write_tree(KEY, &tree(&[("index.html", "hi")])).unwrap();
        a.write_tree(
            "ben/blog/versions/0.0.2/bundle",
            &tree(&[("index.html", "hi")]),
        )
        .unwrap();

        // A directory that does not match the template is ignored, and so is a
        // stray YAML document.
        std::fs::create_dir_all(dir.path().join("ben/blog/versions/0.0.1-dev/collections"))
            .unwrap();
        std::fs::write(dir.path().join("ben/blog/project.yaml"), "label: Blog\n").unwrap();

        let mut keys: Vec<String> = a
            .list_trees()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        keys.sort();
        assert_eq!(keys, vec![KEY, "ben/blog/versions/0.0.2/bundle"]);
    }

    #[test]
    fn store_put_get_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        assert!(!s.has(KEY));

        let inst = s.put(KEY, &tree(&[("index.html", "v1")])).unwrap();
        assert_eq!(inst.project, "ben/blog");
        assert_eq!(inst.version, "0.0.1-dev");
        assert!(s.has(KEY));
        assert_eq!(
            s.read_file(KEY, "index.html").unwrap(),
            Some(b"v1".to_vec())
        );
        assert_eq!(s.list_files(KEY).unwrap(), Some(vec!["index.html".into()]));

        // A reloaded store sees the tree that is on disk.
        let reloaded = store(dir.path());
        assert!(reloaded.has(KEY));
        assert_eq!(reloaded.list_all().len(), 1);

        s.delete(KEY).unwrap();
        assert!(!s.has(KEY));
        assert!(matches!(s.delete(KEY), Err(Error::NotFound(_))));
        assert!(s.list_files(KEY).unwrap().is_none());
    }

    #[test]
    fn store_put_rejects_a_key_that_is_not_this_type() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        assert!(matches!(
            s.put("ben/blog/project", &FileTree::new()),
            Err(Error::InvalidPath(_))
        ));
    }

    #[test]
    fn store_prefix_delete_and_copy_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.put(KEY, &tree(&[("index.html", "v1")])).unwrap();
        s.put(
            "ben/shop/versions/0.0.1-dev/bundle",
            &tree(&[("index.html", "shop")]),
        )
        .unwrap();

        let copied = s
            .copy_by_prefix("ben/blog/versions/0.0.1-dev/", "ben/blog/versions/0.0.1/")
            .unwrap();
        assert_eq!(copied, vec!["ben/blog/versions/0.0.1/bundle".to_string()]);
        assert_eq!(
            s.read_file("ben/blog/versions/0.0.1/bundle", "index.html")
                .unwrap(),
            Some(b"v1".to_vec())
        );

        let deleted = s.delete_by_prefix("ben/blog/").unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(!dir.path().join(KEY).exists());
        assert!(s.has("ben/shop/versions/0.0.1-dev/bundle"));
    }
}
