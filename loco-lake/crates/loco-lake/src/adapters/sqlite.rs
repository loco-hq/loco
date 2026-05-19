use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::adapter::DataAdapter;
use crate::error::Error;
use crate::record::{InsertRequest, Record, UpdatePatch};
use crate::value::Value;

pub struct SqliteAdapter {
    conn: Mutex<Connection>,
}

impl SqliteAdapter {
    pub fn new(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(path).map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS records (
                dataset_id TEXT NOT NULL,
                collection TEXT NOT NULL,
                id         TEXT NOT NULL,
                created_at TEXT NOT NULL,
                created_by TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                updated_by TEXT NOT NULL,
                owner      TEXT NOT NULL,
                fields     TEXT NOT NULL,
                PRIMARY KEY (dataset_id, collection, id)
            )",
        )
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(SqliteAdapter { conn: Mutex::new(conn) })
    }

    fn write_record(&self, dataset_id: &str, collection: &str, record: &Record) -> Result<(), Error> {
        let fields_json =
            serde_json::to_string(&record.fields).map_err(|e| Error::Internal(e.to_string()))?;
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO records (dataset_id, collection, id, created_at, created_by, updated_at, updated_by, owner, fields)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                dataset_id,
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
        Ok(())
    }
}

impl DataAdapter for SqliteAdapter {
    fn insert(
        &self,
        dataset_id: &str,
        collection: &str,
        req: InsertRequest,
    ) -> Result<Record, Error> {
        let record = Record::new_for_insert(dataset_id, req);
        self.write_record(dataset_id, collection, &record)?;
        Ok(record)
    }

    fn get(&self, dataset_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, created_by, updated_at, updated_by, owner, fields
                 FROM records WHERE dataset_id = ?1 AND collection = ?2 AND id = ?3",
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        let record = stmt
            .query_row(rusqlite::params![dataset_id, collection, id], |row| {
                let fields_json: String = row.get(6)?;
                let fields: HashMap<String, Value> =
                    serde_json::from_str(&fields_json).unwrap_or_default();
                Ok(Record {
                    id: row.get(0)?,
                    dataset_id: dataset_id.to_string(),
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
        dataset_id: &str,
        collection: &str,
        id: &str,
        patch: UpdatePatch,
    ) -> Result<Record, Error> {
        let existing = self
            .get(dataset_id, collection, id)?
            .ok_or(Error::NotFound)?;
        let record = existing.apply_patch(patch);

        let fields_json =
            serde_json::to_string(&record.fields).map_err(|e| Error::Internal(e.to_string()))?;
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;

        let rows = conn
            .execute(
                "UPDATE records SET updated_at = ?1, updated_by = ?2, fields = ?3
                 WHERE dataset_id = ?4 AND collection = ?5 AND id = ?6",
                rusqlite::params![
                    record.updated_at,
                    record.updated_by,
                    fields_json,
                    dataset_id,
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

    fn delete(&self, dataset_id: &str, collection: &str, id: &str) -> Result<(), Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;

        let rows = conn
            .execute(
                "DELETE FROM records WHERE dataset_id = ?1 AND collection = ?2 AND id = ?3",
                rusqlite::params![dataset_id, collection, id],
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        if rows == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    fn list(&self, dataset_id: &str, collection: &str) -> Result<Vec<Record>, Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, created_by, updated_at, updated_by, owner, fields
                 FROM records WHERE dataset_id = ?1 AND collection = ?2",
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![dataset_id, collection], |row| {
                let fields_json: String = row.get(6)?;
                let fields: HashMap<String, Value> =
                    serde_json::from_str(&fields_json).unwrap_or_default();
                Ok(Record {
                    id: row.get(0)?,
                    dataset_id: dataset_id.to_string(),
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

    fn delete_dataset(&self, dataset_id: &str) -> Result<(), Error> {
        let conn = self.conn.lock().map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "DELETE FROM records WHERE dataset_id = ?1",
            rusqlite::params![dataset_id],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATASET: &str = "test-dataset";

    fn make_adapter() -> SqliteAdapter {
        SqliteAdapter::new(Path::new(":memory:")).unwrap()
    }

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
        let adapter = make_adapter();
        let inserted = adapter.insert(DATASET, "users", make_request("Alice")).unwrap();

        let retrieved = adapter.get(DATASET, "users", &inserted.id).unwrap().unwrap();
        assert_eq!(retrieved.id, inserted.id);
        assert_eq!(retrieved.fields.get("name").unwrap(), &Value::String("Alice".to_string()));
    }

    #[test]
    fn test_get_missing() {
        let adapter = make_adapter();
        let result = adapter.get(DATASET, "users", "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update_merges_fields_and_stamps_updated_by() {
        let adapter = make_adapter();
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

        assert_eq!(updated.fields.get("name").unwrap(), &Value::String("Bob".to_string()));
        assert_eq!(updated.fields.get("age").unwrap(), &Value::Integer(30));
        assert_eq!(updated.created_by, "creator");
        assert_eq!(updated.owner, "creator");
        assert_eq!(updated.updated_by, "editor");
    }

    #[test]
    fn test_update_missing() {
        let adapter = make_adapter();
        let result = adapter.update(DATASET, "users", "1", make_patch("Alice"));
        assert!(result.is_err());
    }

    #[test]
    fn test_delete() {
        let adapter = make_adapter();
        let inserted = adapter.insert(DATASET, "users", make_request("Alice")).unwrap();
        adapter.delete(DATASET, "users", &inserted.id).unwrap();
        let result = adapter.get(DATASET, "users", &inserted.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_missing() {
        let adapter = make_adapter();
        let result = adapter.delete(DATASET, "users", "1");
        assert!(result.is_err());
    }

    #[test]
    fn test_list() {
        let adapter = make_adapter();
        adapter.insert(DATASET, "users", make_request("Alice")).unwrap();
        adapter.insert(DATASET, "users", make_request("Bob")).unwrap();

        let records = adapter.list(DATASET, "users").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_list_empty() {
        let adapter = make_adapter();
        let records = adapter.list(DATASET, "users").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_dataset_isolation() {
        let adapter = make_adapter();
        let inserted = adapter.insert("dataset-a", "users", make_request("Alice")).unwrap();

        let result = adapter.get("dataset-b", "users", &inserted.id).unwrap();
        assert!(result.is_none());

        let list = adapter.list("dataset-b", "users").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_delete_dataset() {
        let adapter = make_adapter();
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
        let adapter = make_adapter();
        adapter.delete_dataset("nonexistent").unwrap();
    }
}
