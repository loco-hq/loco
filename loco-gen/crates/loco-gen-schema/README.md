# loco-gen-schema

YAML schema parsing, instance loading, and Rust code generation.

## What It Does

1. Parses YAML type definitions into `TypeDef` structs
2. Generates Rust source code — per-type structs with constructors and accessors, plus a `SchemaStore` with typed CRUD methods over a `SchemaRegistry`
3. At runtime, loads instance YAML files from disk, validates them against their type definitions, and serves CRUD operations from a thread-safe in-memory registry

## Type Definitions

Type files live in `schemas/types/` and define a type's properties and on-disk layout:

```yaml
# schemas/types/collection.yaml
description: "A named collection of items"
filePathTemplate: "${project}/versions/${version}/collections/${name}.yaml"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  version:
    type: slug
    createOnly: true
  name:
    type: slug
    createOnly: true
  label:
    type: string
  label_plural:
    type: string
```

### Supported field types

| Type | Notes |
|------|-------|
| `string` | |
| `integer` | `i64` |
| `float` | `f64` |
| `boolean` | |
| `slug` | Path-safe identifier `[a-z0-9_.-]+`. Optional `segments:` (default 1) controls how many `/`-separated parts are allowed. |
| `list` | Requires `items:` naming a scalar type. Nested lists are rejected. |

### Property flags

- `createOnly: true` — field is immutable after creation.
- Every `${var}` in `filePathTemplate` **must** be declared as a property with `type: slug` and `createOnly: true`. Parse fails otherwise.

## Instance Files

Instances live under `schemas/instances/` at paths matching their type's `filePathTemplate`:

```yaml
# schemas/instances/ben/crm/versions/0.0.1/collections/account.yaml
label: "Account"
label_plural: "Accounts"
```

Template-variable fields (`project`, `version`, `name` above) are extracted from the file path — don't repeat them in the YAML body.

### Namespace

An instance's namespace IS its path relative to `instances_dir` with `.yaml` stripped:

- `ben/crm/project.yaml` → `ben/crm/project`
- `ben/crm/versions/0.0.1/collections/account.yaml` → `ben/crm/versions/0.0.1/collections/account`

## Public API

### Parsing

```rust
let type_def = parser::parse_schema(yaml_str, "collection")?;
let type_def = parser::parse_schema_file(path)?;
```

### Instance scanning (low-level)

```rust
let instances = instance::scan_all(instances_dir, &type_defs)?;
```

Most consumers won't call this directly — it's wrapped by `SchemaRegistry::load`.

### Registry (runtime)

`SchemaRegistry` is the thread-safe in-memory store, backed by `RwLock<HashMap<...>>`. The generated `SchemaStore` wraps it; you rarely construct one directly.

```rust
let registry = registry::SchemaRegistry::load(instances_dir, &type_defs)?;
registry.list_all_instances("collection");
registry.get_instance("collection", "ben/crm/versions/0.0.1/collections/account");
registry.create_instance("collection", key, fields)?;
registry.update_instance("collection", key, fields)?;
registry.delete_instance("collection", key)?;
registry.delete_instances_by_prefix("field", prefix)?;
```

Mutating calls write the instance YAML back to disk.

### Code generation

```rust
let code = codegen::generate_all(&type_defs);
```

## Generated Code

For each `TypeDef` named e.g. `Collection`, codegen emits:

- `pub struct Collection { ... }` deriving `Debug, Clone, PartialEq, serde::Serialize`
- `Collection::new(...)` — constructor taking all fields in declaration order
- Field accessors returning `&str` / `i64` / `f64` / `bool` / `&[T]`

Plus, on a single shared `SchemaStore`:

- `SchemaStore::load(instances_dir) -> Result<Self, Error>` — scan and load all instances
- Generic methods keyed by type name: `list_all`, `list`, `get`, `create`, `update`, `delete`
- Typed per-type methods (shown here for `Collection`):
  - `get_collection(key) -> Option<Collection>`
  - `has_collection(key) -> bool`
  - `list_collections(prefix) -> Vec<(String, Collection)>`
  - `list_all_collections() -> Vec<(String, Collection)>`
  - `create_collection(key, fields) -> Result<..>`
  - `update_collection(key, fields) -> Result<..>`
  - `delete_collection(key) -> Result<()>`
  - `delete_collections_by_prefix(prefix) -> Result<Vec<String>>`

Rust keyword escaping is handled automatically (e.g. `type` → `r#type`).

## Key Types

- `TypeDef` — parsed type definition (name, description, `file_path_template`, properties)
- `Property` — name, `FieldType`, `create_only` flag
- `FieldType` — `String`, `Integer`, `Float`, `Boolean`, `Slug { segments }`, `List(Box<FieldType>)`
- `FieldValue` — matching value variant used by `Instance`
- `Instance` — validated instance (type_name, namespace, values)
- `SchemaRegistry` — runtime instance store (thread-safe, on-disk backed)
- `Error` — crate error type

## Tests

```bash
cargo test -p loco-gen-schema
```
