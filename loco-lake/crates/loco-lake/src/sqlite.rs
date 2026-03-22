use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::adapter::DataAdapter;
use crate::error::Error;
use crate::record::Record;
use crate::value::Value;

pub struct SqliteAdapter {
    conn: Mutex<Connection>,
}

impl SqliteAdapter {
    pub fn new(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(path).map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS records (
                tenant_id  TEXT NOT NULL,
                collection TEXT NOT NULL,
                id         TEXT NOT NULL,
                created_at TEXT NOT NULL,
                created_by TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                updated_by TEXT NOT NULL,
                owner      TEXT NOT NULL,
                fields     TEXT NOT NULL,
                PRIMARY KEY (tenant_id, collection, id)
            )",
        )
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(SqliteAdapter { conn: Mutex::new(conn) })
    }
}

impl DataAdapter for SqliteAdapter {
    fn insert(&self, tenant_id: &str, collection: &str, record: Record) -> Result<Record, Error> {
        let fields_json =
            serde_json::to_string(&record.fields).map_err(|e| Error::Internal(e.to_string()))?;
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;

        conn.execute(
            "INSERT INTO records (tenant_id, collection, id, created_at, created_by, updated_at, updated_by, owner, fields)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                tenant_id,
                collection,
                record.id,
                record.created_at,
                record.created_by,
                record.updated_at,
                record.updated_by,
                record.owner,
                fields_json,
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Error::AlreadyExists
            }
            _ => Error::Internal(e.to_string()),
        })?;

        Ok(record)
    }

    fn get(&self, tenant_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, created_by, updated_at, updated_by, owner, fields
                 FROM records WHERE tenant_id = ?1 AND collection = ?2 AND id = ?3",
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        let record = stmt
            .query_row(rusqlite::params![tenant_id, collection, id], |row| {
                let fields_json: String = row.get(6)?;
                let fields: HashMap<String, Value> =
                    serde_json::from_str(&fields_json).unwrap_or_default();
                Ok(Record {
                    id: row.get(0)?,
                    tenant_id: Some(tenant_id.to_string()),
                    created_at: row.get(1)?,
                    created_by: row.get(2)?,
                    updated_at: row.get(3)?,
                    updated_by: row.get(4)?,
                    owner: row.get(5)?,
                    fields,
                })
            })
            .ok();

        Ok(record)
    }

    fn update(
        &self,
        tenant_id: &str,
        collection: &str,
        id: &str,
        record: Record,
    ) -> Result<Record, Error> {
        let fields_json =
            serde_json::to_string(&record.fields).map_err(|e| Error::Internal(e.to_string()))?;
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;

        let rows = conn
            .execute(
                "UPDATE records SET created_at = ?1, created_by = ?2, updated_at = ?3, updated_by = ?4, owner = ?5, fields = ?6
                 WHERE tenant_id = ?7 AND collection = ?8 AND id = ?9",
                rusqlite::params![
                    record.created_at,
                    record.created_by,
                    record.updated_at,
                    record.updated_by,
                    record.owner,
                    fields_json,
                    tenant_id,
                    collection,
                    id,
                ],
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        if rows == 0 {
            return Err(Error::NotFound);
        }

        Ok(record)
    }

    fn delete(&self, tenant_id: &str, collection: &str, id: &str) -> Result<(), Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;

        let rows = conn
            .execute(
                "DELETE FROM records WHERE tenant_id = ?1 AND collection = ?2 AND id = ?3",
                rusqlite::params![tenant_id, collection, id],
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        if rows == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    fn list(&self, tenant_id: &str, collection: &str) -> Result<Vec<Record>, Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, created_by, updated_at, updated_by, owner, fields
                 FROM records WHERE tenant_id = ?1 AND collection = ?2",
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![tenant_id, collection], |row| {
                let fields_json: String = row.get(6)?;
                let fields: HashMap<String, Value> =
                    serde_json::from_str(&fields_json).unwrap_or_default();
                Ok(Record {
                    id: row.get(0)?,
                    tenant_id: Some(tenant_id.to_string()),
                    created_at: row.get(1)?,
                    created_by: row.get(2)?,
                    updated_at: row.get(3)?,
                    updated_by: row.get(4)?,
                    owner: row.get(5)?,
                    fields,
                })
            })
            .map_err(|e| Error::Internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TENANT: &str = "test-tenant";

    fn make_adapter() -> SqliteAdapter {
        SqliteAdapter::new(Path::new(":memory:")).unwrap()
    }

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
        let adapter = make_adapter();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();

        let retrieved = adapter.get(TENANT, "users", "1").unwrap().unwrap();
        assert_eq!(retrieved.id, "1");
        assert_eq!(retrieved.fields.get("name").unwrap(), &Value::String("Alice".to_string()));
    }

    #[test]
    fn test_insert_duplicate() {
        let adapter = make_adapter();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        let result = adapter.insert(TENANT, "users", make_record("1", "Bob"));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_missing() {
        let adapter = make_adapter();
        let result = adapter.get(TENANT, "users", "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update() {
        let adapter = make_adapter();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        adapter.update(TENANT, "users", "1", make_record("1", "Bob")).unwrap();

        let retrieved = adapter.get(TENANT, "users", "1").unwrap().unwrap();
        assert_eq!(retrieved.fields.get("name").unwrap(), &Value::String("Bob".to_string()));
    }

    #[test]
    fn test_update_missing() {
        let adapter = make_adapter();
        let result = adapter.update(TENANT, "users", "1", make_record("1", "Alice"));
        assert!(result.is_err());
    }

    #[test]
    fn test_delete() {
        let adapter = make_adapter();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        adapter.delete(TENANT, "users", "1").unwrap();
        let result = adapter.get(TENANT, "users", "1").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_missing() {
        let adapter = make_adapter();
        let result = adapter.delete(TENANT, "users", "1");
        assert!(result.is_err());
    }

    #[test]
    fn test_list() {
        let adapter = make_adapter();
        adapter.insert(TENANT, "users", make_record("1", "Alice")).unwrap();
        adapter.insert(TENANT, "users", make_record("2", "Bob")).unwrap();

        let records = adapter.list(TENANT, "users").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_list_empty() {
        let adapter = make_adapter();
        let records = adapter.list(TENANT, "users").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_tenant_isolation() {
        let adapter = make_adapter();
        adapter.insert("tenant-a", "users", make_record("1", "Alice")).unwrap();

        let result = adapter.get("tenant-b", "users", "1").unwrap();
        assert!(result.is_none());

        let list = adapter.list("tenant-b", "users").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_same_id_different_tenants() {
        let adapter = make_adapter();
        adapter.insert("tenant-a", "users", make_record("1", "Alice")).unwrap();
        adapter.insert("tenant-b", "users", make_record("1", "Bob")).unwrap();

        let a = adapter.get("tenant-a", "users", "1").unwrap().unwrap();
        let b = adapter.get("tenant-b", "users", "1").unwrap().unwrap();

        assert_eq!(a.fields.get("name").unwrap(), &Value::String("Alice".to_string()));
        assert_eq!(b.fields.get("name").unwrap(), &Value::String("Bob".to_string()));
    }
}
