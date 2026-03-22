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

1. `build.rs` calls `loco_gen_codegen_build::generate("schemas/types", "schemas/instances")`
2. Type definitions (`schemas/types/*.yaml`) are parsed into `TypeDef` structs
3. Instances (`schemas/instances/{user}/{project}/{type}/**/*.yaml`) are recursively scanned and validated against their type
4. Rust code is generated to `$OUT_DIR/loco_generated.rs` — structs, constructors, accessors, cache methods, and baked-in instance loaders
5. `main.rs` includes the generated code via `include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"))`

### Namespace convention

Instance namespace = `{user}/{project}.{key}` where key is the relative path from the type folder minus `.yaml`.
- Flat types (collection): `ben/crm.account`
- Nested types (field with `filePathTemplate`): `ben/crm.account/company`

## Schema Files

Type definitions live in `schemas/types/` with these supported field types: `string`, `integer`, `float`, `boolean`. Optional `filePathTemplate` controls nested instance organization.

Instance files live under `schemas/instances/{user}/{project}/{type}/`. The type folder name is matched case-insensitively to a TypeDef name.

## Key Patterns

- **Rust keyword escaping**: Codegen emits `r#type` (etc.) for property names that are Rust keywords. See `rust_ident()` in `codegen.rs`.
- **Error types**: Each crate has its own error enum — `loco_gen_schema::Error`, `loco_lake::Error`.
- **Tests**: All unit tests are co-located with their module (`#[cfg(test)] mod tests`). Filesystem tests use `tempfile`.
- **Thread safety**: Both `TypedCache` and `InMemoryAdapter` use `RwLock<HashMap<...>>`.

## Rust Edition

2021 — all crates.
