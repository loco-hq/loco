# loco-lake

Multi-tenant data layer providing a pluggable storage abstraction for CRUD operations on schemaless records.

## DataAdapter Trait

The core abstraction. All methods are scoped by `tenant_id` first, then `collection`:

```rust
pub trait DataAdapter: Send + Sync {
    fn insert(&self, tenant_id: &str, collection: &str, record: Record) -> Result<Record, Error>;
    fn get(&self, tenant_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error>;
    fn update(&self, tenant_id: &str, collection: &str, id: &str, record: Record) -> Result<Record, Error>;
    fn delete(&self, tenant_id: &str, collection: &str, id: &str) -> Result<(), Error>;
    fn list(&self, tenant_id: &str, collection: &str) -> Result<Vec<Record>, Error>;
}
```

## Adapters

### InMemoryAdapter

Hash map-based storage using `RwLock<HashMap<String, HashMap<String, Record>>>`. Tenant isolation via composite key (`{tenant_id}::{collection}`). Data does not survive restarts.

```rust
let adapter = InMemoryAdapter::new();
```

### SqliteAdapter

Persistent storage backed by a single SQLite file. Uses a `records` table with a composite primary key of `(tenant_id, collection, id)`. The `fields` column stores the record's field map as JSON. Thread-safe via `Mutex<Connection>`.

```rust
let adapter = SqliteAdapter::new(Path::new("loco.db"))?;
```

## Record

```rust
pub struct Record {
    pub id: String,
    pub tenant_id: Option<String>,
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
    InvalidTenant(String),
    Internal(String),
}
```

## Tests

```bash
cargo test -p loco-lake
```

Both adapters have identical test suites covering CRUD operations and tenant isolation.
