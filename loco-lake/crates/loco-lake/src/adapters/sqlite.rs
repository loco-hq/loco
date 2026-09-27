use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::adapter::DataAdapter;
use crate::error::Error;
use crate::query::{self, CompareOp, Direction, FieldRef, Filter, Kind, LakeQuery, Page};
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

        Ok(SqliteAdapter {
            conn: Mutex::new(conn),
        })
    }

    fn write_record(
        &self,
        dataset_id: &str,
        collection: &str,
        record: &Record,
    ) -> Result<(), Error> {
        let fields_json =
            serde_json::to_string(&record.fields).map_err(|e| Error::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;

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
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;

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
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "DELETE FROM records WHERE dataset_id = ?1",
            rusqlite::params![dataset_id],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }

    fn query(&self, dataset_id: &str, queries: &[LakeQuery]) -> Result<Vec<Page>, Error> {
        for q in queries {
            q.validate()?;
        }
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        // One deferred transaction: every SELECT below reads the same snapshot.
        let tx = conn
            .transaction()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut pages = Vec::with_capacity(queries.len());
        for q in queries {
            pages.push(run_query(&tx, dataset_id, q)?);
        }
        tx.commit().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(pages)
    }
}

fn run_query(conn: &Connection, dataset_id: &str, q: &LakeQuery) -> Result<Page, Error> {
    let order = q.effective_order();
    let mut sql = Sql::default();
    sql.push(
        "SELECT id, created_at, created_by, updated_at, updated_by, owner, fields
         FROM records WHERE dataset_id = ? AND collection = ?",
    );
    sql.bind(dataset_id.to_string().into());
    sql.bind(q.collection.clone().into());
    if let Some(filter) = &q.filter {
        sql.push(" AND ");
        sql.filter(filter);
    }
    if let Some(after) = &q.after {
        sql.push(" AND ");
        sql.after(&order, after);
    }
    sql.push(" ORDER BY ");
    for (i, key) in order.iter().enumerate() {
        if i > 0 {
            sql.push(", ");
        }
        let dir = match key.dir {
            Direction::Asc => " ASC",
            Direction::Desc => " DESC",
        };
        sql.kind(&key.field);
        sql.push(dir);
        sql.push(", ");
        sql.val(&key.field);
        sql.push(dir);
    }
    sql.push(" LIMIT ?");
    let limit = i64::try_from(q.limit.saturating_add(1)).unwrap_or(i64::MAX);
    sql.bind(limit.into());

    let mut stmt = conn
        .prepare(&sql.text)
        .map_err(|e| Error::Internal(e.to_string()))?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(sql.params.iter()), |row| {
            Ok((
                Record {
                    id: row.get(0)?,
                    dataset_id: dataset_id.to_string(),
                    created_at: row.get(1)?,
                    created_by: row.get(2)?,
                    updated_at: row.get(3)?,
                    updated_by: row.get(4)?,
                    owner: row.get(5)?,
                    fields: HashMap::new(),
                },
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| Error::Internal(e.to_string()))?;
    let mut records = Vec::new();
    for row in rows {
        let (mut record, fields_json) = row.map_err(|e| Error::Internal(e.to_string()))?;
        record.fields = serde_json::from_str(&fields_json)
            .map_err(|e| Error::Internal(format!("record {}: {e}", record.id)))?;
        records.push(record);
    }
    Ok(query::finish_page(q, &order, records))
}

/// SQL text plus its positional parameters, built together so each `?`
/// lines up with its value. Values and field names are only ever bound.
#[derive(Default)]
struct Sql {
    text: String,
    params: Vec<rusqlite::types::Value>,
}

impl Sql {
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
    }

    fn bind(&mut self, value: rusqlite::types::Value) {
        self.params.push(value);
    }

    fn path(name: &str) -> rusqlite::types::Value {
        // Quoted so `.` and `[` stay part of the key. Inside the quotes
        // sqlite unescapes `\\` and `\"`. NUL is rejected by
        // `LakeQuery::validate`: sqlite cuts the key there, escaped or not.
        let mut path = String::from("$.\"");
        for c in name.chars() {
            match c {
                '\\' => path.push_str("\\\\"),
                '"' => path.push_str("\\\""),
                c => path.push(c),
            }
        }
        path.push('"');
        path.into()
    }

    /// The field's `query::Kind` as an integer. Never NULL.
    fn kind(&mut self, field: &FieldRef) {
        match field {
            // Not a bare integer: in ORDER BY that would be a column index.
            FieldRef::System(_) => self.push(&format!("CAST({} AS INTEGER)", Kind::String as i64)),
            FieldRef::Field(name) => {
                self.push(&format!(
                    "(CASE json_type(fields, ?) \
                     WHEN 'true' THEN {b} WHEN 'false' THEN {b} \
                     WHEN 'integer' THEN {n} WHEN 'real' THEN {n} \
                     WHEN 'text' THEN {s} ELSE {z} END)",
                    b = Kind::Boolean as i64,
                    n = Kind::Number as i64,
                    s = Kind::String as i64,
                    z = Kind::Null as i64,
                ));
                self.bind(Self::path(name));
            }
        }
    }

    /// The field's value. Booleans read as 0 / 1, null and missing as NULL.
    fn val(&mut self, field: &FieldRef) {
        match field {
            FieldRef::System(s) => self.push(s.column()),
            FieldRef::Field(name) => {
                self.push("json_extract(fields, ?)");
                self.bind(Self::path(name));
            }
        }
    }

    fn bind_value(&mut self, value: &Value) {
        let v = match value {
            Value::Null => rusqlite::types::Value::Null,
            Value::Boolean(b) => rusqlite::types::Value::Integer(i64::from(*b)),
            Value::Integer(i) => rusqlite::types::Value::Integer(*i),
            Value::Float(f) => rusqlite::types::Value::Real(*f),
            Value::String(s) => rusqlite::types::Value::Text(s.clone()),
        };
        self.bind(v);
    }

    /// `field` is `value` under `op`, within `value`'s kind. `op` is `=`
    /// for equality. Null has only equality: its kind is null.
    fn cmp_in_kind(&mut self, field: &FieldRef, op: &str, value: &Value) {
        let k = query::kind(value);
        self.push("(");
        self.kind(field);
        if k == Kind::Null {
            debug_assert_eq!(
                op, "=",
                "ordering against null is rejected or handled by callers"
            );
            self.push(&format!(" = {})", Kind::Null as i64));
            return;
        }
        self.push(&format!(" = {} AND ", k as i64));
        self.val(field);
        self.push(&format!(" {op} ?"));
        self.bind_value(value);
        self.push(")");
    }

    fn filter(&mut self, filter: &Filter) {
        match filter {
            Filter::Compare { field, op, value } => {
                let sym = match op {
                    CompareOp::Eq | CompareOp::Ne => "=",
                    CompareOp::Lt => "<",
                    CompareOp::Lte => "<=",
                    CompareOp::Gt => ">",
                    CompareOp::Gte => ">=",
                };
                if *op == CompareOp::Ne {
                    self.push("NOT ");
                }
                self.cmp_in_kind(field, sym, value);
            }
            Filter::In { field, values } => {
                self.push("(");
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        self.push(" OR ");
                    }
                    self.cmp_in_kind(field, "=", v);
                }
                self.push(")");
            }
            Filter::Exists { field, exists } => {
                self.push("(");
                self.kind(field);
                self.push(if *exists { " <> " } else { " = " });
                self.push(&(Kind::Null as i64).to_string());
                self.push(")");
            }
            Filter::And(filters) => self.join(filters, " AND ", "1"),
            Filter::Or(filters) => self.join(filters, " OR ", "0"),
            Filter::Not(inner) => {
                self.push("NOT ");
                self.filter(inner);
            }
        }
    }

    fn join(&mut self, filters: &[Filter], sep: &str, empty: &str) {
        if filters.is_empty() {
            self.push(empty);
            return;
        }
        self.push("(");
        for (i, f) in filters.iter().enumerate() {
            if i > 0 {
                self.push(sep);
            }
            self.filter(f);
        }
        self.push(")");
    }

    /// Keyset: the row's order-key tuple sorts strictly after `after`.
    /// `(k0 > a0) OR (k0 = a0 AND k1 > a1) OR …`, each `>` direction-aware
    /// and comparing kind before value, as `query::sort_cmp` does.
    fn after(&mut self, order: &[query::OrderKey], after: &[Value]) {
        self.push("(");
        for i in 0..order.len() {
            if i > 0 {
                self.push(" OR ");
            }
            self.push("(");
            for j in 0..i {
                self.cmp_in_kind(&order[j].field, "=", &after[j]);
                self.push(" AND ");
            }
            self.past(&order[i], &after[i]);
            self.push(")");
        }
        self.push(")");
    }

    fn past(&mut self, key: &query::OrderKey, value: &Value) {
        let (kind_op, val_op) = match key.dir {
            Direction::Asc => (" > ", ">"),
            Direction::Desc => (" < ", "<"),
        };
        let k = query::kind(value);
        self.push("(");
        self.kind(&key.field);
        self.push(&format!("{kind_op}{}", k as i64));
        if k != Kind::Null {
            self.push(" OR ");
            self.cmp_in_kind(&key.field, val_op, value);
        }
        self.push(")");
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
        let inserted = adapter
            .insert(DATASET, "users", make_request("Alice"))
            .unwrap();

        let retrieved = adapter
            .get(DATASET, "users", &inserted.id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.id, inserted.id);
        assert_eq!(
            retrieved.fields.get("name").unwrap(),
            &Value::String("Alice".to_string())
        );
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

        assert_eq!(
            updated.fields.get("name").unwrap(),
            &Value::String("Bob".to_string())
        );
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
        let inserted = adapter
            .insert(DATASET, "users", make_request("Alice"))
            .unwrap();
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
        adapter
            .insert(DATASET, "users", make_request("Alice"))
            .unwrap();
        adapter
            .insert(DATASET, "users", make_request("Bob"))
            .unwrap();

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
        let inserted = adapter
            .insert("dataset-a", "users", make_request("Alice"))
            .unwrap();

        let result = adapter.get("dataset-b", "users", &inserted.id).unwrap();
        assert!(result.is_none());

        let list = adapter.list("dataset-b", "users").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_delete_dataset() {
        let adapter = make_adapter();
        adapter
            .insert("ds", "users", make_request("Alice"))
            .unwrap();
        adapter
            .insert("ds", "orders", make_request("Order1"))
            .unwrap();
        adapter
            .insert("other", "users", make_request("Bob"))
            .unwrap();

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
