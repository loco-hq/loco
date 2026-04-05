# Loco

Schema-driven code generation system that turns YAML type definitions into type-safe Rust structs at build time, paired with an in-memory data layer and REST API server.

## Commands

```bash
cargo test                    # Run all workspace tests
cargo test -p loco-gen-schema # Test schema/codegen crate only
cargo clippy --workspace      # Lint everything
cargo run -p loco-apps        # Run the web server on :3000
cargo run -p basic-example    # Run the codegen demo
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

1. `build.rs` calls `loco_gen_codegen_build::generate("schemas/types", "schemas/instances", "schemas/config")`
2. Type definitions (`schemas/types/*.yaml`) are parsed into `TypeDef` structs
3. Namespaced instances (`schemas/instances/{user}/{project}/{version}/{type}/**/*.yaml`) are scanned for `scope: namespaced` types
4. Global config instances (`schemas/config/{type}/*.yaml`) are scanned for `scope: global` types
5. Rust code is generated to `$OUT_DIR/loco_generated.rs` — structs, constructors, accessors, cache methods, and baked-in instance loaders
6. `main.rs` includes the generated code via `include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"))`

### Namespace convention

Instance namespace = `{user}/{project}.{key}` where key is the relative path from the type folder minus `.yaml`.
- Flat types (collection): `ben/crm.account`
- Nested types (field with `filePathTemplate`): `ben/crm.account/company`

## Schema Files

Type definitions live in `schemas/types/` with these supported field types: `string`, `integer`, `float`, `boolean`. Optional `filePathTemplate` controls nested instance organization. The `scope` field controls instance addressing:
- `scope: namespaced` (default) — instances live under `schemas/instances/{user}/{project}/{version}/{type}/`, addressed as `user/project.name`
- `scope: global` — config instances live under `schemas/config/{type}/`, addressed by simple id (e.g., `studio`)

Namespaced instance files live under `schemas/instances/{user}/{project}/{version}/{type}/`. Global config files live under `schemas/config/{type}/`. Type folder names are matched case-insensitively to TypeDef names.

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
