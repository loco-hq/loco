use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

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
pub struct InstanceStore<T: SchemaInstance> {
    cache: RwLock<BTreeMap<String, Arc<T>>>,
    adapter: Arc<dyn SchemaPersistence<T>>,
}

impl<T: SchemaInstance> InstanceStore<T> {
    pub fn new(adapter: Arc<dyn SchemaPersistence<T>>) -> Self {
        Self {
            cache: RwLock::new(BTreeMap::new()),
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

    pub fn create(&self, value: T) -> Result<Arc<T>, Error> {
        let key = value.to_path();
        {
            let cache = self.cache.read().unwrap();
            if cache.contains_key(&key) {
                return Err(Error::AlreadyExists(key));
            }
        }
        self.adapter.write(&key, &value)?;
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
        self.adapter.write(key, &updated)?;
        let arc = Arc::new(updated);
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
}
