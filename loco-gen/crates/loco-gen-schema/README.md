# loco-gen-schema

YAML type parsing and Rust code generation. Runtime instance storage lives in `loco-schema-runtime`; this crate produces the types that crate stores.

## What it does

1. Parses YAML type definitions into `TypeDef` structs
2. Generates Rust source — per-type structs with constructors, accessors, `to_path` / `from_path` / `from_yaml`, an `Update` patch type, and a `SchemaInstance` impl
3. Emits a `SchemaStore` that owns one `loco_schema_runtime::InstanceStore<T>` per type, each backed by `YamlFsAdapter`

Call it from a crate's `build.rs`:

```rust
fn main() {
    loco_gen_schema::build::generate("schemas/types");
}
```

That writes `$OUT_DIR/loco_generated.rs`. Instances are not scanned at build time.

## Type definitions

Type files live in `schemas/types/` and define a type's properties and on-disk layout:

```yaml
# schemas/types/collection.yaml
description: "A named collection of items"
pathTemplate: "${project}/versions/${version}/collections/${name}"
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
- Every `${var}` in `pathTemplate` **must** be declared as a property with `type: slug` and `createOnly: true`. Parse fails otherwise.

## Instance files

Instances live under `schemas/instances/` at paths matching their type's `pathTemplate`, with `.yaml` appended. Template-variable fields are extracted from the file path — don't repeat them in the YAML body.

An instance's key **is** its path relative to `instances_dir` with `.yaml` stripped.

Loading and persistence are `SchemaStore::load` / `InstanceStore` in the generated code, backed by `loco_schema_runtime`. This crate no longer has a `SchemaRegistry`.

## Generated code

For each `TypeDef` named e.g. `Collection`, codegen emits:

- `pub struct Collection { ... }` deriving `Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize`
- `Collection::new(...)` — constructor taking all fields in declaration order
- Field accessors
- `to_path` / `from_path` / `from_yaml`
- `CollectionUpdate` patch type
- `impl SchemaInstance for Collection`
- `type CollectionStore = InstanceStore<Collection>`

Plus a shared `SchemaStore` with `load(instances_dir)` and per-type accessors (`schema.collections()`, …). Each store exposes typed CRUD: `get`, `has`, `list`, `list_all`, `create`, `update`, `delete`, `delete_by_prefix`.

Rust keyword escaping is handled automatically (e.g. `type` → `r#type`).

## Key types

- `TypeDef` — parsed type definition (name, description, `path_template`, properties)
- `Property` — name, `FieldType`, `create_only` flag
- `FieldType` — `String`, `Integer`, `Float`, `Boolean`, `Slug { segments }`, `List(Box<FieldType>)`
- `Error` — crate error type

## Tests

```bash
cargo test -p loco-gen-schema
```
