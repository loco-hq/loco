use std::collections::HashMap;
use std::sync::RwLock;

use crate::adapter::DataAdapter;
use crate::error::Error;
use crate::record::{InsertRequest, Record, UpdatePatch};

pub struct InMemoryAdapter {
    store: RwLock<HashMap<String, HashMap<String, Record>>>,
}

impl InMemoryAdapter {
    pub fn new() -> Self {
        InMemoryAdapter {
            store: RwLock::new(HashMap::new()),
        }
    }

    fn scoped_key(dataset_id: &str, collection: &str) -> String {
        format!("{dataset_id}::{collection}")
    }
}

impl Default for InMemoryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DataAdapter for InMemoryAdapter {
    fn insert(
        &self,
        dataset_id: &str,
        collection: &str,
        req: InsertRequest,
    ) -> Result<Record, Error> {
        let record = Record::new_for_insert(dataset_id, req);
        let key = Self::scoped_key(dataset_id, collection);
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        let coll = store.entry(key).or_default();
        if coll.contains_key(&record.id) {
            return Err(Error::AlreadyExists);
        }
        coll.insert(record.id.clone(), record.clone());
        Ok(record)
    }

    fn get(&self, dataset_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error> {
        let key = Self::scoped_key(dataset_id, collection);
        let store = self.store.read().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(store.get(&key).and_then(|coll| coll.get(id).cloned()))
    }

    fn update(
        &self,
        dataset_id: &str,
        collection: &str,
        id: &str,
        patch: UpdatePatch,
    ) -> Result<Record, Error> {
        let key = Self::scoped_key(dataset_id, collection);
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        let coll = store.get_mut(&key).ok_or(Error::NotFound)?;
        let existing = coll.get(id).cloned().ok_or(Error::NotFound)?;
        let record = existing.apply_patch(patch);
        coll.insert(id.to_string(), record.clone());
        Ok(record)
    }

    fn delete(&self, dataset_id: &str, collection: &str, id: &str) -> Result<(), Error> {
        let key = Self::scoped_key(dataset_id, collection);
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        let coll = store.get_mut(&key).ok_or(Error::NotFound)?;
        coll.remove(id).ok_or(Error::NotFound)?;
        Ok(())
    }

    fn list(&self, dataset_id: &str, collection: &str) -> Result<Vec<Record>, Error> {
        let key = Self::scoped_key(dataset_id, collection);
        let store = self.store.read().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(store
            .get(&key)
            .map(|coll| coll.values().cloned().collect())
            .unwrap_or_default())
    }

    fn delete_dataset(&self, dataset_id: &str) -> Result<(), Error> {
        let prefix = format!("{dataset_id}::");
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        store.retain(|key, _| !key.starts_with(&prefix));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    const DATASET: &str = "test-dataset";

    fn make_request(name: &str) -> InsertRequest {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), Value::String(name.to_string()));
        InsertRequest {
            user: "test".to_string(),
            fields,
        }
    }

    fn make_patch(name: &str) -> UpdatePatch {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), Value::String(name.to_string()));
        UpdatePatch {
            user: "test".to_string(),
            fields,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let adapter = InMemoryAdapter::new();
        let inserted = adapter.insert(DATASET, "users", make_request("Alice")).unwrap();

        let retrieved = adapter.get(DATASET, "users", &inserted.id).unwrap().unwrap();
        assert_eq!(retrieved.id, inserted.id);
        assert_eq!(retrieved.fields.get("name").unwrap(), &Value::String("Alice".to_string()));
        assert_eq!(retrieved.created_by, "test");
        assert_eq!(retrieved.owner, "test");
    }

    #[test]
    fn test_get_missing() {
        let adapter = InMemoryAdapter::new();
        let result = adapter.get(DATASET, "users", "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update_merges_fields_and_stamps_updated_by() {
        let adapter = InMemoryAdapter::new();
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), Value::String("Alice".to_string()));
        fields.insert("age".to_string(), Value::Integer(30));
        let inserted = adapter
            .insert(
                DATASET,
                "users",
                InsertRequest {
                    user: "creator".to_string(),
                    fields,
                },
            )
            .unwrap();

        let mut patch_fields = HashMap::new();
        patch_fields.insert("name".to_string(), Value::String("Bob".to_string()));
        let updated = adapter
            .update(
                DATASET,
                "users",
                &inserted.id,
                UpdatePatch {
                    user: "editor".to_string(),
                    fields: patch_fields,
                },
            )
            .unwrap();

        // patched field overwritten, untouched field preserved
        assert_eq!(updated.fields.get("name").unwrap(), &Value::String("Bob".to_string()));
        assert_eq!(updated.fields.get("age").unwrap(), &Value::Integer(30));
        // creator/owner preserved, updated_by stamped
        assert_eq!(updated.created_by, "creator");
        assert_eq!(updated.owner, "creator");
        assert_eq!(updated.updated_by, "editor");
    }

    #[test]
    fn test_update_missing() {
        let adapter = InMemoryAdapter::new();
        let result = adapter.update(DATASET, "users", "1", make_patch("Alice"));
        assert!(result.is_err());
    }

    #[test]
    fn test_delete() {
        let adapter = InMemoryAdapter::new();
        let inserted = adapter.insert(DATASET, "users", make_request("Alice")).unwrap();
        adapter.delete(DATASET, "users", &inserted.id).unwrap();
        let result = adapter.get(DATASET, "users", &inserted.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_missing() {
        let adapter = InMemoryAdapter::new();
        let result = adapter.delete(DATASET, "users", "1");
        assert!(result.is_err());
    }

    #[test]
    fn test_list() {
        let adapter = InMemoryAdapter::new();
        adapter.insert(DATASET, "users", make_request("Alice")).unwrap();
        adapter.insert(DATASET, "users", make_request("Bob")).unwrap();

        let records = adapter.list(DATASET, "users").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_list_empty() {
        let adapter = InMemoryAdapter::new();
        let records = adapter.list(DATASET, "users").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_dataset_isolation() {
        let adapter = InMemoryAdapter::new();
        let inserted = adapter.insert("dataset-a", "users", make_request("Alice")).unwrap();

        // dataset-b should not see dataset-a's data
        let result = adapter.get("dataset-b", "users", &inserted.id).unwrap();
        assert!(result.is_none());

        let list = adapter.list("dataset-b", "users").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_delete_dataset() {
        let adapter = InMemoryAdapter::new();
        adapter.insert("ds", "users", make_request("Alice")).unwrap();
        adapter.insert("ds", "orders", make_request("Order1")).unwrap();
        adapter.insert("other", "users", make_request("Bob")).unwrap();

        adapter.delete_dataset("ds").unwrap();

        assert!(adapter.list("ds", "users").unwrap().is_empty());
        assert!(adapter.list("ds", "orders").unwrap().is_empty());
        assert_eq!(adapter.list("other", "users").unwrap().len(), 1);
    }

    #[test]
    fn test_delete_dataset_empty() {
        let adapter = InMemoryAdapter::new();
        adapter.delete_dataset("nonexistent").unwrap();
    }
}
