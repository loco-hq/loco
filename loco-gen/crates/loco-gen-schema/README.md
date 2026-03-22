# loco-gen-schema

YAML schema parsing, instance validation, and Rust code generation.

## What It Does

1. Parses YAML type definitions into `TypeDef` structs
2. Scans and validates instance files against their type definitions
3. Generates Rust source code — structs, constructors, accessors, and instance loaders

## Type Definitions

Type files live in `schemas/types/` and define a type's properties:

```yaml
# schemas/types/collection.yaml
version: 1
description: "A named collection of items"
properties:
  name:
    type: string
  label:
    type: string
  label_plural:
    type: string
```

Supported field types: `string`, `integer`, `float`, `boolean`.

Optional `filePathTemplate` controls how nested instances are organized on disk:

```yaml
filePathTemplate: "${collection}/${name}"
```

## Instance Files

Instances live under `schemas/instances/{user}/{project}/{type}/` and are validated against their type definition at scan time. The type folder name is matched case-insensitively to a TypeDef.

```yaml
# schemas/instances/ben/crm/collection/account.yaml
name: "account"
label: "Account"
label_plural: "Accounts"
```

### Namespace Convention

- Flat types: `{user}/{project}.{key}` (e.g., `ben/crm.account`)
- Nested types (with `filePathTemplate`): `{user}/{project}.{subdir}/{key}` (e.g., `ben/crm.account/company`)

## Public API

### Parsing

```rust
// Parse a schema from a YAML string
let schema = parser::parse_schema(yaml_str)?;

// Parse from file
let schema = parser::parse_schema_file(path)?;
```

### Instance Scanning

```rust
// Scan all instances, validating against type definitions
let instances = instance::scan_instances(instances_dir, &type_defs)?;
```

### Code Generation

```rust
// Generate Rust code for all types with their instances
let code = codegen::generate_all(&type_defs, &instances)?;
```

### Generated Code Per Type

For a type named `Collection`, the codegen emits:

- `struct Collection { ... }` with `#[derive(Debug, Clone, PartialEq)]`
- `Collection::new(name, label, label_plural)` — constructor
- `.name()`, `.label()`, `.label_plural()` — accessor methods
- `Collection::from_cache(cache, key)` — reconstruct from `TypedCache`
- `.to_cache(cache, key)` — store into `TypedCache`
- `Collection::load_instance(namespace)` — load a single baked-in instance
- `Collection::load_all_instances()` — load all instances as `Vec<(&str, Self)>`

Rust keyword escaping is handled automatically (e.g., `type` becomes `r#type`).

## Key Types

- `TypeDef` — parsed type definition with name, description, properties
- `Property` — name + `FieldType` pair
- `FieldType` — String, Integer, Float, Boolean
- `Instance` — validated instance with type name, namespace, and field values
- `Schema` — version + TypeDef wrapper

## Tests

```bash
cargo test -p loco-gen-schema
```
