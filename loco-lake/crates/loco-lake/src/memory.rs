use std::collections::HashMap;
use std::sync::RwLock;

use crate::adapter::DataAdapter;
use crate::error::Error;
use crate::record::Record;

pub struct InMemoryAdapter {
    store: RwLock<HashMap<String, HashMap<String, Record>>>,
}

impl InMemoryAdapter {
    pub fn new() -> Self {
        InMemoryAdapter {
            store: RwLock::new(HashMap::new()),
        }
    }

    fn scoped_key(tenant_id: &str, collection: &str) -> String {
        format!("{tenant_id}::{collection}")
    }
}

impl Default for InMemoryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DataAdapter for InMemoryAdapter {
    fn insert(&self, tenant_id: &str, collection: &str, record: Record) -> Result<Record, Error> {
        let key = Self::scoped_key(tenant_id, collection);
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        let coll = store.entry(key).or_default();
        if coll.contains_key(&record.id) {
            return Err(Error::AlreadyExists);
        }
        let id = record.id.clone();
        coll.insert(id.clone(), record);
        Ok(coll.get(&id).map(|r| Record {
            id: r.id.clone(),
            tenant_id: r.tenant_id.clone(),
            created_at: r.created_at.clone(),
            created_by: r.created_by.clone(),
            updated_at: r.updated_at.clone(),
            updated_by: r.updated_by.clone(),
            owner: r.owner.clone(),
            fields: r.fields.clone(),
        }).unwrap())
    }

    fn get(&self, tenant_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error> {
        let key = Self::scoped_key(tenant_id, collection);
        let store = self.store.read().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(store.get(&key).and_then(|coll| {
            coll.get(id).map(|r| Record {
                id: r.id.clone(),
                tenant_id: r.tenant_id.clone(),
                created_at: r.created_at.clone(),
                created_by: r.created_by.clone(),
                updated_at: r.updated_at.clone(),
                updated_by: r.updated_by.clone(),
                owner: r.owner.clone(),
                fields: r.fields.clone(),
            })
        }))
    }

    fn update(&self, tenant_id: &str, collection: &str, id: &str, record: Record) -> Result<Record, Error> {
        let key = Self::scoped_key(tenant_id, collection);
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        let coll = store.get_mut(&key).ok_or(Error::NotFound)?;
        if !coll.contains_key(id) {
            return Err(Error::NotFound);
        }
        coll.insert(id.to_string(), record);
        Ok(coll.get(id).map(|r| Record {
            id: r.id.clone(),
            tenant_id: r.tenant_id.clone(),
            created_at: r.created_at.clone(),
            created_by: r.created_by.clone(),
            updated_at: r.updated_at.clone(),
            updated_by: r.updated_by.clone(),
            owner: r.owner.clone(),
            fields: r.fields.clone(),
        }).unwrap())
    }

    fn delete(&self, tenant_id: &str, collection: &str, id: &str) -> Result<(), Error> {
        let key = Self::scoped_key(tenant_id, collection);
        let mut store = self.store.write().map_err(|e| Error::Internal(e.to_string()))?;
        let coll = store.get_mut(&key).ok_or(Error::NotFound)?;
        coll.remove(id).ok_or(Error::NotFound)?;
        Ok(())
    }

    fn list(&self, tenant_id: &str, collection: &str) -> Result<Vec<Record>, Error> {
        let key = Self::scoped_key(tenant_id, collection);
        let store = self.store.read().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(store.get(&key).map(|coll| {
            coll.values().map(|r| Record {
                id: r.id.clone(),
                tenant_id: r.tenant_id.clone(),
                created_at: r.created_at.clone(),
                created_by: r.created_by.clone(),
                updated_at: r.updated_at.clone(),
                updated_by: r.updated_by.clone(),
                owner: r.owner.clone(),
                fields: r.fields.clone(),
            }).collect()
        }).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    const TENANT: &str = "test-tenant";

    fn make_record(id: &str, name: &str) -> Record {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), Value::String(name.to_string()));
        Record {
            id: id.to_string(),
            tenant_id: None,
            created_at: "2024-01-01".to_string(),
            created_by: "test".to_string(),
            updated_at: "2024-01-01".to_string(),
            updated_by: "test".to_string(),
            owner: "test".to_string(),
            fields,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let adapter = InMemoryAdapter::new();
        let record = make_record("1", "Alice");
        adapter.insert(TENANT, "users", record).unwrap();

        let retrieved = adapter.get(TENANT, "users", "1").unwrap().unwrap();
        assert_eq!(retrieved.id, "1");
        assert_eq!(retrieved.fields.get("name").unwrap(), &Value::String("Alice".to_string()));
    }

    #[test]
    fn test_insert_duplicate() {
        let adapter = InMemoryAdapter::new();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        let result = adapter.insert(TENANT, "users", make_record("1", "Bob"));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_missing() {
        let adapter = InMemoryAdapter::new();
        let result = adapter.get(TENANT, "users", "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update() {
        let adapter = InMemoryAdapter::new();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        adapter.update(TENANT, "users", "1", make_record("1", "Bob")).unwrap();

        let retrieved = adapter.get(TENANT, "users", "1").unwrap().unwrap();
        assert_eq!(retrieved.fields.get("name").unwrap(), &Value::String("Bob".to_string()));
    }

    #[test]
    fn test_update_missing() {
        let adapter = InMemoryAdapter::new();
        let result = adapter.update(TENANT, "users", "1", make_record("1", "Alice"));
        assert!(result.is_err());
    }

    #[test]
    fn test_delete() {
        let adapter = InMemoryAdapter::new();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        adapter.delete(TENANT, "users", "1").unwrap();
        let result = adapter.get(TENANT, "users", "1").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_missing() {
        let adapter = InMemoryAdapter::new();
        let result = adapter.delete(TENANT, "users", "1");
        assert!(result.is_err());
    }

    #[test]
    fn test_list() {
        let adapter = InMemoryAdapter::new();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        adapter.insert(TENANT, "users", make_record("2", "Bob")).unwrap();

        let records = adapter.list(TENANT, "users").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_list_empty() {
        let adapter = InMemoryAdapter::new();
        let records = adapter.list(TENANT, "users").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_tenant_isolation() {
        let adapter = InMemoryAdapter::new();
        adapter.insert("tenant-a", "users", make_record("1", "Alice")).unwrap();

        // tenant-b should not see tenant-a's data
        let result = adapter.get("tenant-b", "users", "1").unwrap();
        assert!(result.is_none());

        let list = adapter.list("tenant-b", "users").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_same_id_different_tenants() {
        let adapter = InMemoryAdapter::new();
        adapter.insert("tenant-a", "users", make_record("1", "Alice")).unwrap();
        adapter.insert("tenant-b", "users", make_record("1", "Bob")).unwrap();

        let a = adapter.get("tenant-a", "users", "1").unwrap().unwrap();
        let b = adapter.get("tenant-b", "users", "1").unwrap().unwrap();

        assert_eq!(a.fields.get("name").unwrap(), &Value::String("Alice".to_string()));
        assert_eq!(b.fields.get("name").unwrap(), &Value::String("Bob".to_string()));
    }
}
