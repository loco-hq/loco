# Loco

Schema-driven code generation system that turns YAML type definitions into type-safe Rust structs at build time, paired with an in-memory data layer and REST API server.

## Commands

```bash
cargo test                    # Run all workspace tests
cargo test -p loco-gen-schema # Test schema/codegen crate only
cargo clippy --workspace      # Lint everything
cargo run -p loco-apps        # Run the web server on :3000
```

## Project Structure

```
loco/
├── loco-gen/crates/
│   ├── loco-gen-schema/         # YAML parsing, SchemaRegistry, Rust codegen
│   └── loco-gen-codegen-build/  # build.rs API — calls schema to emit code
├── loco-lake/crates/loco-lake/  # DataAdapter trait + InMemoryAdapter (CRUD)
└── loco-apps/                   # Axum web server consuming generated types
```

### Dependency flow

Build-time: `loco-apps/build.rs` → `loco-gen-codegen-build` → `loco-gen-schema`
Runtime: `loco-apps` → `loco-gen-schema` + `loco-lake`

## How Codegen Works

1. `build.rs` calls `loco_gen_codegen_build::generate("schemas/types")`
2. Type definitions (`schemas/types/*.yaml`) are parsed into `TypeDef` structs
3. Rust code is generated to `$OUT_DIR/loco_generated.rs` — per-type structs with constructors and accessors, plus a `SchemaStore` with load/list/get/create/update/delete methods over a `SchemaRegistry`
4. `main.rs` includes the generated code via `include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"))`

Instances are **not** scanned at build time. At server startup, `SchemaStore::load("schemas/instances")` walks the instances directory, matches each YAML file against its type's `pathTemplate`, and populates the registry in memory.

### Namespace convention

An instance's namespace IS its path relative to `schemas/instances/` with `.yaml` stripped — it matches the type's `pathTemplate` with values filled in. For example:
- `schemas/instances/ben/crm/project.yaml` → `ben/crm/project`
- `schemas/instances/ben/crm/datasets/acme.yaml` → `ben/crm/datasets/acme`
- `schemas/instances/ben/crm/versions/0.0.1/collections/account.yaml` → `ben/crm/versions/0.0.1/collections/account`

## Schema Files

Type definitions live in `schemas/types/`. Supported field types: `string`, `integer`, `float`, `boolean`, and `list` (a `list` requires an `items:` sub-key naming a scalar type — nested lists are rejected at parse time). Every type has a required `pathTemplate` that controls where instance files live under `schemas/instances/` and how template variables are extracted from file paths. The template is purely logical — it never contains `.yaml`; the storage layer appends that extension when writing to disk.

### pathTemplate examples

| Type | Template |
|------|----------|
| project | `${project}/project` |
| dataset | `${project}/datasets/${name}` |
| site | `${project}/sites/${name}` |
| manifest | `${project}/versions/${version}/manifest` |
| collection | `${project}/versions/${version}/collections/${name}` |
| field | `${project}/versions/${version}/fields/${collection}/${name}` |

`${project}` is a multi-segment variable (e.g., `ben/crm`). Instance files all live under `schemas/instances/`. Hard-coded path segments are always plural (`sites`, `datasets`, `collections`, `fields`, `versions`).

### Manifests

Each versioned project must include a `manifest.yaml` declaring its dependencies. `manifest` is a regular schema type — loco-gen treats it no differently than `collection` or `site`. The dependency grammar (`{user}/{project}@{version}`) and transitive-tree resolution live in loco-apps (`src/manifest.rs`); loco-gen has no concept of dependencies. Missing-dependency validation runs at server startup.

## Naming Conventions

These conventions apply to property names in type definitions and variable names in `pathTemplate`s.

- **`id`** — opaque identifier (uuid/number) with no semantic meaning. Immutable once set. *(Reserved — not currently used anywhere in the codebase.)*
- **`name`** — semantic slug identifier: `[a-z_]` only, lowercase, immutable. Used as path-segment identifiers in `pathTemplate`s. When the template has `${name}`, the struct field is populated implicitly from the path — don't declare `name` as a property.
- **`label`** — human-readable display string. Any characters, short, mutable.
- **`description`** — free-form text. Any characters, longer, mutable.
- **`project`** — fully-qualified project reference (e.g. `ben/crm`). Used for direct "belongs-to" links and in path templates via `${project}`. Preferred in user-facing contexts.
- **`namespace`** — external scope reference, for pulling inherited metadata from another project (e.g. app-store dependency). Reserved for cross-project references; do not use as a synonym for `project`.

### Template variables must be declared

Every `${var}` in a `pathTemplate` **must** be declared as a property with `type: slug` and `createOnly: true`. Parse fails otherwise (`TemplateVarNotDeclared` / `TemplateVarNotSlug` / `TemplateVarNotCreateOnly`). At instance-load time the value is extracted from the file path, so instance YAML bodies should not repeat these fields.

## Key Patterns

- **Rust keyword escaping**: Codegen emits `r#type` (etc.) for property names that are Rust keywords. See `rust_ident()` in `codegen.rs`.
- **Error types**: Each crate has its own error enum — `loco_gen_schema::Error`, `loco_lake::Error`.
- **Tests**: All unit tests are co-located with their module (`#[cfg(test)] mod tests`). Filesystem tests use `tempfile`.
- **Thread safety**: Both `SchemaRegistry` and `InMemoryAdapter` use `RwLock<HashMap<...>>`.

## Frontend Apps

All frontend apps use the same stack:

- **Vite** with `@vitejs/plugin-react`
- **React** (functional components, hooks)
- **React Router** (`react-router-dom`, `createHashRouter`)
- API client in `loco.js` (plain JS, not a hook)
- Components in `src/components/` as `.jsx` files
- Proxy `/api` to `localhost:3000` in vite.config.js

### Frontend locations

- `loco-studio/` — Schema management UI (port 5174)
- `loco-frontend-examples/cars/` — Example app using schema introspection (default port)

## Rust Edition

2021 — all crates.
