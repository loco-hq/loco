use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use crate::adapters::SchemaPersistence;
use crate::error::Error;

/// Implemented by every generated schema type. Gives the store and adapter just
/// enough generic access to do CRUD, write the persisted form, and parse it
/// back into a typed instance — without knowing the type's fields.
pub trait SchemaInstance: Clone + Sized + serde::Serialize + 'static {
    type Update;

    /// Build this instance's persistence key (its namespace / relative path
    /// without `.yaml`).
    fn to_path(&self) -> String;

    /// Apply a partial update in-place. Only `Some` fields are overwritten.
    fn apply_update(&mut self, patch: &Self::Update);

    /// Match a key against this type's `pathTemplate`. Returns the extracted
    /// template variables on success, or `None` if the key belongs to a
    /// different type.
    fn from_path(path: &str) -> Option<HashMap<String, String>>;

    /// Parse the persisted YAML body for this type, merging in path-derived
    /// `vars` for fields that come from the key.
    fn from_yaml(yaml: &str, vars: &HashMap<String, String>) -> Result<Self, Error>;
}

/// Per-type typed cache backed by `RwLock<BTreeMap<String, Arc<T>>>`.
/// Reads hand out `Arc<T>` so in-flight readers are unaffected by concurrent writes.
///
/// All persistence I/O is delegated to the [`SchemaPersistence`] adapter; the
/// store only manages the in-memory index and the read/write coordination.
///
/// # Locking
///
/// Every mutation holds `writer` from its existence check through the adapter
/// write to the cache update, so check-persist-cache is one step: two creates
/// of one key cannot both win, and a read-modify-write through
/// [`Self::update_with`] cannot lose a concurrent change. Readers only take the
/// `cache` lock, briefly, and never wait on disk I/O.
///
/// Lock order is `writer` then `cache`, within one store only. No method here
/// calls into another store, so the stores in a `SchemaStore` never nest
/// their locks and cannot deadlock one another. Keep it that way: a caller
/// that needs several stores takes them one after another, never one inside
/// another.
pub struct InstanceStore<T: SchemaInstance> {
    cache: RwLock<BTreeMap<String, Arc<T>>>,
    writer: Mutex<()>,
    adapter: Arc<dyn SchemaPersistence<T>>,
}

impl<T: SchemaInstance> InstanceStore<T> {
    pub fn new(adapter: Arc<dyn SchemaPersistence<T>>) -> Self {
        Self {
            cache: RwLock::new(BTreeMap::new()),
            writer: Mutex::new(()),
            adapter,
        }
    }

    /// Insert an already-parsed instance into the cache. Used by `SchemaStore::load`.
    pub fn insert_loaded(&self, key: String, instance: Arc<T>) {
        self.cache.write().unwrap().insert(key, instance);
    }

    pub fn get(&self, key: &str) -> Option<Arc<T>> {
        self.cache.read().unwrap().get(key).cloned()
    }

    pub fn has(&self, key: &str) -> bool {
        self.cache.read().unwrap().contains_key(key)
    }

    /// All instances whose key starts with `prefix`. Uses a `BTreeMap` range scan.
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

    /// Serializes mutations. It guards no data of its own, so a panic while
    /// it was held leaves nothing inconsistent and the poison is ignored.
    fn lock_writer(&self) -> MutexGuard<'_, ()> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn create(&self, value: T) -> Result<Arc<T>, Error> {
        let key = value.to_path();
        let _writer = self.lock_writer();
        if self.has(&key) {
            return Err(Error::AlreadyExists(key));
        }
        let written = persisted(self.adapter.write(&key, &value))?;
        let arc = Arc::new(value);
        self.cache.write().unwrap().insert(key, arc.clone());
        written.map(|()| arc)
    }

    pub fn update(&self, key: &str, patch: T::Update) -> Result<Arc<T>, Error> {
        self.update_with(key, |_| Some(patch))
    }

    /// Read-modify-write one instance atomically with respect to every other
    /// mutation of this store. `f` sees the current value and returns the
    /// patch to apply, or `None` to leave it as is (nothing is written, and
    /// the current value is returned).
    ///
    /// `f` runs with this store's writer lock held: it must not call back
    /// into this store, and must not mutate any other store.
    pub fn update_with<F>(&self, key: &str, f: F) -> Result<Arc<T>, Error>
    where
        F: FnOnce(&T) -> Option<T::Update>,
    {
        let _writer = self.lock_writer();
        let current = self
            .get(key)
            .ok_or_else(|| Error::NotFound(key.to_string()))?;
        let Some(patch) = f(&current) else {
            return Ok(current);
        };
        let mut updated: T = (*current).clone();
        updated.apply_update(&patch);
        let written = persisted(self.adapter.write(key, &updated))?;
        let arc = Arc::new(updated);
        self.cache
            .write()
            .unwrap()
            .insert(key.to_string(), arc.clone());
        written.map(|()| arc)
    }

    pub fn delete(&self, key: &str) -> Result<(), Error> {
        let _writer = self.lock_writer();
        if !self.has(key) {
            return Err(Error::NotFound(key.to_string()));
        }
        self.adapter.delete(key)?;
        self.cache.write().unwrap().remove(key);
        Ok(())
    }

    /// Delete every instance whose key starts with `prefix`, returning the
    /// deleted keys. Every key is attempted; one whose adapter delete fails
    /// stays in the cache (it is still on disk) and the first such error is
    /// returned once the rest are done, so cache and disk agree either way.
    pub fn delete_by_prefix(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let _writer = self.lock_writer();
        let keys: Vec<String> = self.list(prefix).into_iter().map(|(k, _)| k).collect();
        let mut deleted = Vec::new();
        let mut first_err = None;
        for key in keys {
            match self.adapter.delete(&key) {
                Ok(()) => {
                    self.cache.write().unwrap().remove(&key);
                    deleted.push(key);
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(deleted),
        }
    }
}

/// Sort an adapter write's result into "did not happen" (the outer `Err`: skip
/// the cache update) and "happened" (the inner result: update the cache, then
/// return it). [`Error::NotDurable`] is the one error that means the new
/// contents are already what readers of the file see.
fn persisted(result: Result<(), Error>) -> Result<Result<(), Error>, Error> {
    match result {
        Err(e @ Error::NotDurable(_)) => Ok(Err(e)),
        Err(e) => Err(e),
        Ok(()) => Ok(Ok(())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::YamlFsAdapter;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestItem {
        project: String,
        name: String,
        label: String,
    }

    #[derive(Debug, Default)]
    struct TestItemUpdate {
        label: Option<String>,
    }

    impl SchemaInstance for TestItem {
        type Update = TestItemUpdate;
        fn to_path(&self) -> String {
            format!("{}/items/{}", self.project, self.name)
        }
        fn apply_update(&mut self, patch: &Self::Update) {
            if let Some(v) = &patch.label {
                self.label = v.clone();
            }
        }
        fn from_path(path: &str) -> Option<HashMap<String, String>> {
            let segs: Vec<&str> = path.split('/').collect();
            if segs.len() != 4 || segs[2] != "items" {
                return None;
            }
            let mut vars = HashMap::new();
            vars.insert("project".to_string(), format!("{}/{}", segs[0], segs[1]));
            vars.insert("name".to_string(), segs[3].to_string());
            Some(vars)
        }
        fn from_yaml(yaml: &str, vars: &HashMap<String, String>) -> Result<Self, Error> {
            let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;
            Ok(TestItem {
                project: vars.get("project").cloned().unwrap_or_default(),
                name: vars.get("name").cloned().unwrap_or_default(),
                label: value
                    .get("label")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default(),
            })
        }
    }

    fn fresh_store() -> (tempfile::TempDir, InstanceStore<TestItem>) {
        let dir = tempfile::tempdir().unwrap();
        let adapter: Arc<dyn SchemaPersistence<TestItem>> =
            Arc::new(YamlFsAdapter::new(dir.path().to_path_buf()));
        let store = InstanceStore::new(adapter);
        (dir, store)
    }

    fn sample(project: &str, name: &str, label: &str) -> TestItem {
        TestItem {
            project: project.to_string(),
            name: name.to_string(),
            label: label.to_string(),
        }
    }

    #[test]
    fn create_and_get() {
        let (dir, store) = fresh_store();

        let v = store
            .create(sample("ben/crm", "account", "Account"))
            .unwrap();
        assert_eq!(v.label, "Account");

        let got = store.get("ben/crm/items/account").unwrap();
        assert!(Arc::ptr_eq(&v, &got));

        let yaml_path = dir.path().join("ben/crm/items/account.yaml");
        assert!(yaml_path.exists());
    }

    #[test]
    fn duplicate_create_fails() {
        let (_dir, store) = fresh_store();
        store.create(sample("ben/crm", "a", "A")).unwrap();
        assert!(matches!(
            store.create(sample("ben/crm", "a", "A")),
            Err(Error::AlreadyExists(_))
        ));
    }

    #[test]
    fn update_merges_and_writes() {
        let (_dir, store) = fresh_store();
        store.create(sample("ben/crm", "a", "A")).unwrap();

        let updated = store
            .update(
                "ben/crm/items/a",
                TestItemUpdate {
                    label: Some("A2".to_string()),
                },
            )
            .unwrap();
        assert_eq!(updated.label, "A2");
        assert_eq!(updated.name, "a");
    }

    #[test]
    fn update_missing_fails() {
        let (_dir, store) = fresh_store();
        let res = store.update(
            "ben/crm/items/missing",
            TestItemUpdate {
                label: Some("x".to_string()),
            },
        );
        assert!(matches!(res, Err(Error::NotFound(_))));
    }

    #[test]
    fn delete_and_prefix() {
        let (_dir, store) = fresh_store();
        store.create(sample("ben/crm", "a", "A")).unwrap();
        store.create(sample("ben/crm", "b", "B")).unwrap();
        store.create(sample("ben/cars", "x", "X")).unwrap();

        let deleted = store.delete_by_prefix("ben/crm/").unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(!store.has("ben/crm/items/a"));
        assert!(store.has("ben/cars/items/x"));

        store.delete("ben/cars/items/x").unwrap();
        assert!(!store.has("ben/cars/items/x"));
        assert!(matches!(
            store.delete("ben/cars/items/x"),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn list_uses_prefix() {
        let (_dir, store) = fresh_store();
        store.create(sample("ben/crm", "a", "A")).unwrap();
        store.create(sample("ben/crm", "b", "B")).unwrap();
        store.create(sample("ben/cars", "x", "X")).unwrap();

        let crm = store.list("ben/crm/");
        assert_eq!(crm.len(), 2);
        let all = store.list_all();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn concurrent_create_of_one_key_has_one_winner() {
        const N: usize = 16;
        let (dir, store) = fresh_store();
        let store = Arc::new(store);
        let barrier = Arc::new(std::sync::Barrier::new(N));
        let results: Vec<Result<Arc<TestItem>, Error>> = (0..N)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.create(sample("ben/crm", "a", &format!("A{i}")))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();

        let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(winners.len(), 1);
        assert!(results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| matches!(r, Err(Error::AlreadyExists(_)))));

        // The file on disk is the winner's, and so is the cache.
        let on_disk = std::fs::read_to_string(dir.path().join("ben/crm/items/a.yaml")).unwrap();
        assert!(on_disk.contains(&winners[0].label));
        assert_eq!(
            store.get("ben/crm/items/a").unwrap().label,
            winners[0].label
        );
    }

    #[test]
    fn concurrent_update_with_loses_nothing() {
        const N: usize = 16;
        let (_dir, store) = fresh_store();
        let store = Arc::new(store);
        store.create(sample("ben/crm", "a", "")).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(N));
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store
                        .update_with("ben/crm/items/a", |cur| {
                            Some(TestItemUpdate {
                                label: Some(format!("{}{},", cur.label, i)),
                            })
                        })
                        .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let label = store.get("ben/crm/items/a").unwrap().label.clone();
        assert_eq!(label.split_terminator(',').count(), N);
    }

    #[test]
    fn update_with_none_writes_nothing() {
        let (dir, store) = fresh_store();
        let created = store.create(sample("ben/crm", "a", "A")).unwrap();
        let path = dir.path().join("ben/crm/items/a.yaml");
        std::fs::remove_file(&path).unwrap();
        let got = store.update_with("ben/crm/items/a", |_| None).unwrap();
        assert!(Arc::ptr_eq(&created, &got));
        assert!(!path.exists());
    }

    /// Writes succeed; deletes fail for any key containing `fail`.
    struct FailingDeletes;

    impl SchemaPersistence<TestItem> for FailingDeletes {
        fn load_all(&self) -> Result<Vec<(String, TestItem)>, Error> {
            Ok(Vec::new())
        }
        fn write(&self, _key: &str, _value: &TestItem) -> Result<(), Error> {
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), Error> {
            if key.contains("fail") {
                Err(Error::Io(std::io::Error::other("disk says no")))
            } else {
                Ok(())
            }
        }
    }

    /// Every write lands but reports that the directory flush failed.
    struct UnflushedWrites;

    impl SchemaPersistence<TestItem> for UnflushedWrites {
        fn load_all(&self) -> Result<Vec<(String, TestItem)>, Error> {
            Ok(Vec::new())
        }
        fn write(&self, _key: &str, _value: &TestItem) -> Result<(), Error> {
            Err(Error::NotDurable(std::io::Error::other("fsync failed")))
        }
        fn delete(&self, _key: &str) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn not_durable_write_still_updates_the_cache_and_reports() {
        let store = InstanceStore::new(Arc::new(UnflushedWrites));
        assert!(matches!(
            store.create(sample("ben/crm", "a", "A")),
            Err(Error::NotDurable(_))
        ));
        assert_eq!(store.get("ben/crm/items/a").unwrap().label, "A");

        let res = store.update(
            "ben/crm/items/a",
            TestItemUpdate {
                label: Some("A2".to_string()),
            },
        );
        assert!(matches!(res, Err(Error::NotDurable(_))));
        assert_eq!(store.get("ben/crm/items/a").unwrap().label, "A2");
    }

    #[test]
    fn delete_by_prefix_reports_failure_and_keeps_undeleted_keys() {
        let store = InstanceStore::new(Arc::new(FailingDeletes));
        store.create(sample("ben/crm", "a", "A")).unwrap();
        store.create(sample("ben/crm", "fail", "F")).unwrap();
        store.create(sample("ben/crm", "z", "Z")).unwrap();

        assert!(matches!(
            store.delete_by_prefix("ben/crm/"),
            Err(Error::Io(_))
        ));
        // Keys on either side of the failure are gone; the one still on disk
        // is still cached.
        assert!(!store.has("ben/crm/items/a"));
        assert!(store.has("ben/crm/items/fail"));
        assert!(!store.has("ben/crm/items/z"));
    }
}
