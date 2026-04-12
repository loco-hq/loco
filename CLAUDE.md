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
│   ├── loco-gen-schema/         # YAML parsing, instance scanning, Rust codegen
│   ├── loco-gen-runtime/        # Minimal runtime (TypedCache, Value) — zero deps
│   └── loco-gen-codegen-build/  # build.rs API — calls schema to emit code
├── loco-lake/crates/loco-lake/  # DataAdapter trait + InMemoryAdapter (CRUD)
└── loco-apps/                   # Axum web server consuming generated types
```

### Dependency flow

Build-time: `loco-apps/build.rs` → `loco-gen-codegen-build` → `loco-gen-schema`
Runtime: `loco-apps` → `loco-gen-runtime` + `loco-lake`

## How Codegen Works

1. `build.rs` calls `loco_gen_codegen_build::generate("schemas/types", "schemas/instances")`
2. Type definitions (`schemas/types/*.yaml`) are parsed into `TypeDef` structs
3. All instance YAML files under `schemas/instances/` are matched against each type's `filePathTemplate` to determine their type and extract template variables
4. Rust code is generated to `$OUT_DIR/loco_generated.rs` — structs, constructors, accessors, cache methods, and baked-in instance loaders
5. `main.rs` includes the generated code via `include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"))`

### Namespace convention

Instance namespace depends on whether the type's template includes `${version}`:
- Versioned types (collection, field): `{project}.{item_key}` — e.g., `ben/crm.account`
- Unversioned types (project, dataset, site): relative path minus `.yaml` — e.g., `ben/crm/project`, `ben/crm/datasets/acme`

## Schema Files

Type definitions live in `schemas/types/`. Supported field types: `string`, `integer`, `float`, `boolean`, and `list` (a `list` requires an `items:` sub-key naming a scalar type — nested lists are rejected at parse time). Every type has a required `filePathTemplate` that controls where instance files live under `schemas/instances/` and how template variables are extracted from file paths.

### filePathTemplate examples

| Type | Template |
|------|----------|
| project | `${project}/project.yaml` |
| dataset | `${project}/datasets/${name}.yaml` |
| site | `${project}/sites/${name}.yaml` |
| manifest | `${project}/versions/${version}/manifest.yaml` |
| collection | `${project}/versions/${version}/collections/${name}.yaml` |
| field | `${project}/versions/${version}/fields/${collection}/${name}.yaml` |

`${project}` is a multi-segment variable (e.g., `ben/crm`). Instance files all live under `schemas/instances/`. Hard-coded path segments are always plural (`sites`, `datasets`, `collections`, `fields`, `versions`).

### Manifests

Each versioned project must include a `manifest.yaml` declaring its dependencies. `manifest` is a regular schema type — loco-gen treats it no differently than `collection` or `site`. The dependency grammar (`{user}/{project}@{version}`) and transitive-tree resolution live in loco-apps (`src/manifest.rs`); loco-gen has no concept of dependencies. Missing-dependency validation runs at server startup.

## Naming Conventions

These conventions apply to property names in type definitions and variable names in `filePathTemplate`s.

- **`id`** — opaque identifier (uuid/number) with no semantic meaning. Immutable once set. *(Reserved — not currently used anywhere in the codebase.)*
- **`name`** — semantic slug identifier: `[a-z_]` only, lowercase, immutable. Used as path-segment identifiers in `filePathTemplate`s. When the template has `${name}`, the struct field is populated implicitly from the path — don't declare `name` as a property.
- **`label`** — human-readable display string. Any characters, short, mutable.
- **`description`** — free-form text. Any characters, longer, mutable.
- **`project`** — fully-qualified project reference (e.g. `ben/crm`). Used for direct "belongs-to" links and in path templates via `${project}`. Preferred in user-facing contexts.
- **`namespace`** — external scope reference, for pulling inherited metadata from another project (e.g. app-store dependency). Reserved for cross-project references; do not use as a synonym for `project`.

### Implicit fields from template variables

Every `${var}` in a `filePathTemplate` becomes an implicit `String` field on the generated struct. A declared property whose name collides with a template variable is a parse-time error — the template is the source of truth for those values.

## Key Patterns

- **Rust keyword escaping**: Codegen emits `r#type` (etc.) for property names that are Rust keywords. See `rust_ident()` in `codegen.rs`.
- **Error types**: Each crate has its own error enum — `loco_gen_schema::Error`, `loco_lake::Error`.
- **Tests**: All unit tests are co-located with their module (`#[cfg(test)] mod tests`). Filesystem tests use `tempfile`.
- **Thread safety**: Both `TypedCache` and `InMemoryAdapter` use `RwLock<HashMap<...>>`.

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
