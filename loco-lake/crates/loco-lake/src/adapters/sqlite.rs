use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, Row};

use crate::adapter::DataAdapter;
use crate::error::Error;
use crate::query::{
    self, Collation, CompareOp, Direction, FieldRef, Filter, Kind, LakeQuery, Page,
};
use crate::record::{InsertRequest, Record, UpdatePatch};
use crate::value::Value;

/// The registered name of `query::natural_cmp`. Not `natural`: that is a
/// keyword (`NATURAL JOIN`).
const NATURAL: &str = "loco_natural";

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
        // `Collation::Natural`: the memory adapter's comparator, verbatim.
        conn.create_collation(NATURAL, query::natural_cmp)
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

    fn upsert(
        &self,
        dataset_id: &str,
        collection: &str,
        id: &str,
        user: &str,
        fields: HashMap<String, Value>,
    ) -> Result<Record, Error> {
        // One lock for the read and the write. `write_record` takes the same
        // mutex, so the SQL stays inline here.
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM records WHERE dataset_id = ?1 AND collection = ?2 AND id = ?3"
            ))
            .map_err(|e| Error::Internal(e.to_string()))?;
        let existing = stmt
            .query_row(rusqlite::params![dataset_id, collection, id], |row| {
                read_row(row, dataset_id)
            })
            .optional()
            .map_err(|e| Error::Internal(e.to_string()))?;
        drop(stmt);

        let replacing = existing.is_some();
        let record = if let Some(existing) = existing {
            decode(existing)?.apply_patch(UpdatePatch {
                user: user.to_string(),
                fields,
            })
        } else {
            Record::with_id(dataset_id, id, user, fields)
        };
        let fields_json =
            serde_json::to_string(&record.fields).map_err(|e| Error::Internal(e.to_string()))?;
        if replacing {
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
        } else {
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
            .map_err(|e| Error::Internal(e.to_string()))?;
        }
        Ok(record)
    }

    fn get(&self, dataset_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM records WHERE dataset_id = ?1 AND collection = ?2 AND id = ?3"
            ))
            .map_err(|e| Error::Internal(e.to_string()))?;

        // Only no row is `None`. A locked database or an unreadable row is
        // an error, not a 404.
        stmt.query_row(rusqlite::params![dataset_id, collection, id], |row| {
            read_row(row, dataset_id)
        })
        .optional()
        .map_err(|e| Error::Internal(e.to_string()))?
        .map(decode)
        .transpose()
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
            .prepare(&format!(
                "SELECT {COLUMNS} FROM records WHERE dataset_id = ?1 AND collection = ?2"
            ))
            .map_err(|e| Error::Internal(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![dataset_id, collection], |row| {
                read_row(row, dataset_id)
            })
            .map_err(|e| Error::Internal(e.to_string()))?;
        decode_all(rows)
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

/// The columns `read_row` reads, in its order. Every record SELECT uses it.
const COLUMNS: &str = "id, created_at, created_by, updated_at, updated_by, owner, fields";

/// A row selected as `COLUMNS`: the record with its `fields` JSON still
/// unparsed, since a rusqlite row callback can only fail with a rusqlite
/// error. `decode` finishes it.
fn read_row(row: &Row, dataset_id: &str) -> rusqlite::Result<(Record, String)> {
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
        row.get(6)?,
    ))
}

/// Parses a row's `fields`. Malformed JSON is an error naming the record,
/// never empty fields: those would read as data that is not there.
fn decode((mut record, fields_json): (Record, String)) -> Result<Record, Error> {
    record.fields = serde_json::from_str(&fields_json)
        .map_err(|e| Error::Internal(format!("record {}: {e}", record.id)))?;
    Ok(record)
}

fn decode_all(
    rows: impl Iterator<Item = rusqlite::Result<(Record, String)>>,
) -> Result<Vec<Record>, Error> {
    rows.map(|row| decode(row.map_err(|e| Error::Internal(e.to_string()))?))
        .collect()
}

fn run_query(conn: &Connection, dataset_id: &str, q: &LakeQuery) -> Result<Page, Error> {
    let order = q.effective_order();
    let mut sql = Sql::default();
    sql.push(&format!(
        "SELECT {COLUMNS} FROM records WHERE dataset_id = ? AND collection = ?"
    ));
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
        sql.val(&key.field, key.collation);
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
            read_row(row, dataset_id)
        })
        .map_err(|e| Error::Internal(e.to_string()))?;
    let records = decode_all(rows)?;
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
    /// `collation` applies when it is compared with a string; sqlite ignores
    /// it for numbers.
    fn val(&mut self, field: &FieldRef, collation: Collation) {
        match field {
            FieldRef::System(s) => self.push(s.column()),
            FieldRef::Field(name) => {
                self.push("json_extract(fields, ?)");
                self.bind(Self::path(name));
            }
        }
        if collation == Collation::Natural {
            self.push(&format!(" COLLATE {NATURAL}"));
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

    /// `field` is `value` under `op`, within `value`'s kind, strings compared
    /// under `collation`. `op` is `=` for equality. Null has only equality:
    /// its kind is null.
    fn cmp_in_kind(&mut self, field: &FieldRef, op: &str, value: &Value, collation: Collation) {
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
        self.val(field, collation);
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
                self.cmp_in_kind(field, sym, value, Collation::Binary);
            }
            Filter::In { field, values } => {
                self.push("(");
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        self.push(" OR ");
                    }
                    self.cmp_in_kind(field, "=", v, Collation::Binary);
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
    /// and comparing kind before value, then strings under the key's
    /// collation, as `query::sort_cmp` does. The ORDER BY uses the same
    /// collation, so a page resumes exactly where the last one stopped.
    fn after(&mut self, order: &[query::OrderKey], after: &[Value]) {
        self.push("(");
        for i in 0..order.len() {
            if i > 0 {
                self.push(" OR ");
            }
            self.push("(");
            for j in 0..i {
                self.cmp_in_kind(&order[j].field, "=", &after[j], order[j].collation);
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
            self.cmp_in_kind(&key.field, val_op, value, key.collation);
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

    /// Inserts Alice, then overwrites one column of her row directly.
    fn corrupt(adapter: &SqliteAdapter, column: &str, value: rusqlite::types::Value) -> String {
        let id = adapter
            .insert(DATASET, "users", make_request("Alice"))
            .unwrap()
            .id;
        adapter
            .conn
            .lock()
            .unwrap()
            .execute(
                &format!("UPDATE records SET {column} = ?1 WHERE id = ?2"),
                rusqlite::params![value, id],
            )
            .unwrap();
        id
    }

    fn assert_internal<T: std::fmt::Debug>(result: Result<T, Error>, needle: &str) {
        match result {
            Err(Error::Internal(msg)) => assert!(msg.contains(needle), "{msg}"),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[test]
    fn test_malformed_fields_is_internal_naming_the_record() {
        let adapter = make_adapter();
        let id = corrupt(&adapter, "fields", "{not json".to_string().into());

        assert_internal(adapter.get(DATASET, "users", &id), &id);
        assert_internal(adapter.list(DATASET, "users"), &id);
        assert_internal(adapter.query(DATASET, &[LakeQuery::new("users", 10)]), &id);
        assert_internal(
            adapter.update(DATASET, "users", &id, make_patch("Bob")),
            &id,
        );
    }

    #[test]
    fn test_unreadable_row_is_internal_not_missing() {
        // A blob where a string belongs (TEXT affinity would convert an
        // integer): a rusqlite error other than no rows, which `get` used
        // to answer as `None`.
        let adapter = make_adapter();
        let id = corrupt(&adapter, "created_at", vec![0xff_u8].into());

        assert_internal(adapter.get(DATASET, "users", &id), "");
        assert_internal(adapter.list(DATASET, "users"), "");
        assert_internal(adapter.query(DATASET, &[LakeQuery::new("users", 10)]), "");
        assert!(adapter
            .get(DATASET, "users", "nonexistent")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_delete_dataset_empty() {
        let adapter = make_adapter();
        adapter.delete_dataset("nonexistent").unwrap();
    }

    #[test]
    fn test_upsert_keeps_the_caller_id() {
        let adapter = make_adapter();
        let id = "alice/bricklink.consumer_key";
        let mut fields = HashMap::new();
        fields.insert("nonce".into(), Value::String("one".into()));
        let created = adapter
            .upsert(DATASET, "$secrets", id, "system", fields)
            .unwrap();
        assert_eq!(created.id, id);

        let mut fields = HashMap::new();
        fields.insert("nonce".into(), Value::String("two".into()));
        let updated = adapter
            .upsert(DATASET, "$secrets", id, "system", fields)
            .unwrap();
        assert_eq!(updated.id, id);
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(adapter.list(DATASET, "$secrets").unwrap().len(), 1);
        assert_eq!(
            adapter
                .get(DATASET, "$secrets", id)
                .unwrap()
                .unwrap()
                .fields
                .get("nonce"),
            Some(&Value::String("two".into()))
        );
    }
}
