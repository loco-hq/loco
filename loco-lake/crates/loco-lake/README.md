# loco-lake

Schemaless record store. All methods are scoped by `dataset_id`, then `collection`. Validation, versions, and sites live in `loco-apps` — this crate does not know about schemas.

## DataAdapter

```rust
pub trait DataAdapter: Send + Sync {
    fn insert(&self, dataset_id: &str, collection: &str, req: InsertRequest) -> Result<Record, Error>;
    fn get(&self, dataset_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error>;
    fn update(&self, dataset_id: &str, collection: &str, id: &str, patch: UpdatePatch) -> Result<Record, Error>;
    fn delete(&self, dataset_id: &str, collection: &str, id: &str) -> Result<(), Error>;
    fn list(&self, dataset_id: &str, collection: &str) -> Result<Vec<Record>, Error>;
    fn delete_dataset(&self, dataset_id: &str) -> Result<(), Error>;
}
```

`InsertRequest` / `UpdatePatch` carry `user` + `fields`. The adapter stamps `id`, timestamps, `created_by` / `updated_by` / `owner`.

`dataset_id` is typically `{user}/{project}/{dataset_name}` (see `SiteScope::dataset_id` in loco-apps). This crate treats it as an opaque string.

## Adapters

### InMemoryAdapter

`RwLock<HashMap<…>>`. Data does not survive restarts. Used by the Hurl suites (`LOCO_ADAPTER=memory`).

```rust
let adapter = InMemoryAdapter::new();
```

### SqliteAdapter

One SQLite file. `records` table with primary key `(dataset_id, collection, id)`. `fields` is JSON. Thread-safe via `Mutex<Connection>`.

```rust
let adapter = SqliteAdapter::new(Path::new("loco.db"))?;
```

## Record

```rust
pub struct Record {
    pub id: String,
    pub dataset_id: String,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    pub owner: String,
    pub fields: HashMap<String, Value>,
}
```

## Value

```rust
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
}
```

## Error

```rust
pub enum Error {
    NotFound,
    AlreadyExists,
    InvalidDataset(String),
    Internal(String),
}
```

## Tests

```bash
cargo test -p loco-lake
```

Both adapters have matching CRUD + dataset-isolation suites.
