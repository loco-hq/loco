use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::error::Error;
use crate::io;

/// Implemented by every generated schema type. Gives the store just enough
/// generic access to do CRUD and write YAML without knowing the type's fields.
pub trait SchemaInstance: Clone + Sized + serde::Serialize + 'static {
    type Update;

    /// Build this instance's on-disk key (its namespace / relative path without `.yaml`).
    fn to_path(&self) -> String;

    /// Apply a partial update in-place. Only `Some` fields are overwritten.
    fn apply_update(&mut self, patch: &Self::Update);
}

/// Per-type typed cache backed by `RwLock<BTreeMap<String, Arc<T>>>`.
/// Reads hand out `Arc<T>` so in-flight readers are unaffected by concurrent writes.
pub struct InstanceStore<T: SchemaInstance> {
    cache: RwLock<BTreeMap<String, Arc<T>>>,
    instances_dir: PathBuf,
}

impl<T: SchemaInstance> InstanceStore<T> {
    pub fn new(instances_dir: PathBuf) -> Self {
        Self {
            cache: RwLock::new(BTreeMap::new()),
            instances_dir,
        }
    }

    /// Insert an already-parsed instance into the cache. Used by `SchemaStore::load`.
    pub fn insert_loaded(&self, namespace: String, instance: Arc<T>) {
        self.cache.write().unwrap().insert(namespace, instance);
    }

    pub fn instances_dir(&self) -> &Path {
        &self.instances_dir
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

    pub fn create(&self, value: T) -> Result<Arc<T>, Error> {
        let key = value.to_path();
        {
            let cache = self.cache.read().unwrap();
            if cache.contains_key(&key) {
                return Err(Error::AlreadyExists(key));
            }
        }
        let file_path = self.resolve_file_path(&key);
        io::write_yaml(&file_path, &value)?;
        let arc = Arc::new(value);
        self.cache.write().unwrap().insert(key, arc.clone());
        Ok(arc)
    }

    pub fn update(&self, key: &str, patch: T::Update) -> Result<Arc<T>, Error> {
        let current = self
            .get(key)
            .ok_or_else(|| Error::NotFound(key.to_string()))?;
        let mut updated: T = (*current).clone();
        updated.apply_update(&patch);
        let file_path = self.resolve_file_path(key);
        io::write_yaml(&file_path, &updated)?;
        let arc = Arc::new(updated);
        self.cache.write().unwrap().insert(key.to_string(), arc.clone());
        Ok(arc)
    }

    pub fn delete(&self, key: &str) -> Result<(), Error> {
        {
            let cache = self.cache.read().unwrap();
            if !cache.contains_key(key) {
                return Err(Error::NotFound(key.to_string()));
            }
        }
        let file_path = self.resolve_file_path(key);
        io::delete_yaml(&file_path, &self.instances_dir)?;
        self.cache.write().unwrap().remove(key);
        Ok(())
    }

    pub fn delete_by_prefix(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let to_delete: Vec<String> = {
            let cache = self.cache.read().unwrap();
            cache
                .range(prefix.to_string()..)
                .take_while(|(k, _)| k.starts_with(prefix))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for ns in &to_delete {
            let file_path = self.resolve_file_path(ns);
            let _ = io::delete_yaml(&file_path, &self.instances_dir);
        }
        let mut cache = self.cache.write().unwrap();
        for ns in &to_delete {
            cache.remove(ns);
        }
        Ok(to_delete)
    }

    fn resolve_file_path(&self, namespace: &str) -> PathBuf {
        self.instances_dir.join(format!("{namespace}.yaml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = tempfile::tempdir().unwrap();
        let store: InstanceStore<TestItem> = InstanceStore::new(dir.path().to_path_buf());

        let v = store.create(sample("ben/crm", "account", "Account")).unwrap();
        assert_eq!(v.label, "Account");

        let got = store.get("ben/crm/items/account").unwrap();
        assert!(Arc::ptr_eq(&v, &got));

        let yaml_path = dir.path().join("ben/crm/items/account.yaml");
        assert!(yaml_path.exists());
    }

    #[test]
    fn duplicate_create_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store: InstanceStore<TestItem> = InstanceStore::new(dir.path().to_path_buf());
        store.create(sample("ben/crm", "a", "A")).unwrap();
        assert!(matches!(
            store.create(sample("ben/crm", "a", "A")),
            Err(Error::AlreadyExists(_))
        ));
    }

    #[test]
    fn update_merges_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store: InstanceStore<TestItem> = InstanceStore::new(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let store: InstanceStore<TestItem> = InstanceStore::new(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let store: InstanceStore<TestItem> = InstanceStore::new(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let store: InstanceStore<TestItem> = InstanceStore::new(dir.path().to_path_buf());
        store.create(sample("ben/crm", "a", "A")).unwrap();
        store.create(sample("ben/crm", "b", "B")).unwrap();
        store.create(sample("ben/cars", "x", "X")).unwrap();

        let crm = store.list("ben/crm/");
        assert_eq!(crm.len(), 2);
        let all = store.list_all();
        assert_eq!(all.len(), 3);
    }
}
