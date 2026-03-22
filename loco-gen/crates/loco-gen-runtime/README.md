# loco-gen-runtime

Minimal runtime library for generated code. Zero external dependencies.

## Purpose

Generated Rust structs depend on this crate for caching and value representation. It is intentionally kept lightweight so that consumers of generated code don't pull in the full schema/codegen machinery.

## TypedCache

Thread-safe key-value store used by generated `from_cache`/`to_cache` methods:

```rust
let cache = TypedCache::new();

// Store a map of values
let mut fields = HashMap::new();
fields.insert("name".to_string(), Value::String("Alice".into()));
cache.set("user:1", fields);

// Retrieve
let data = cache.get("user:1"); // Option<HashMap<String, Value>>

// Check and remove
cache.contains("user:1"); // true
cache.remove("user:1");
```

Internally backed by `RwLock<HashMap<String, HashMap<String, Value>>>` for concurrent read/write access.

## Value

```rust
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}
```

With typed accessors: `.as_string()`, `.as_integer()`, `.as_float()`, `.as_boolean()` — each returns `Option<&T>` or `Option<T>`.
