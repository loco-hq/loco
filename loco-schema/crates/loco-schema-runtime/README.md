# loco-schema-runtime

Typed in-memory store for schema instances, plus a YAML-on-disk persistence adapter. Generated types from `loco-gen-schema` implement `SchemaInstance`; `SchemaStore::load` constructs one `InstanceStore<T>` per type.

This crate has no knowledge of projects, versions, or manifests. Scoping lives in `loco-apps`.

## SchemaInstance

Implemented by every generated type:

```rust
pub trait SchemaInstance: Clone + Sized + serde::Serialize + 'static {
    type Update;
    fn to_path(&self) -> String;
    fn apply_update(&mut self, patch: &Self::Update);
    fn from_path(path: &str) -> Option<HashMap<String, String>>;
    fn from_yaml(yaml: &str, vars: &HashMap<String, String>) -> Result<Self, Error>;
}
```

## InstanceStore

`RwLock<BTreeMap<String, Arc<T>>>` plus a `SchemaPersistence<T>` adapter. Reads return `Arc<T>` so in-flight readers are unaffected by writes. Prefix list is a BTree range scan.

`create` / `update` / `delete` / `delete_by_prefix` persist through the adapter, then update the cache.

## YamlFsAdapter

Keys map to `{instances_dir}/{key}.yaml`. `load_all` walks the tree, skips files whose path does not match `T::from_path`, and merges path-derived template vars into the parsed body. Empty parent dirs are pruned on delete.

## Tests

```bash
cargo test -p loco-schema-runtime
```
