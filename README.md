# Loco

Schema-driven code generation system that turns YAML type definitions into type-safe Rust structs at build time, paired with a multi-tenant data layer and REST API server.

## Quick Start

```bash
# Run the web server
cd loco-apps && cargo run

# Run all tests
cargo test

# Lint
cargo clippy --workspace
```

The server starts on `http://localhost:3000`.

## Architecture

```
loco/
├── loco-gen/crates/
│   ├── loco-gen-schema/           # YAML parsing, instance scanning, Rust codegen
│   ├── loco-gen-runtime/          # Minimal runtime (TypedCache, Value) — zero deps
│   └── loco-gen-codegen-build/    # build.rs API — calls schema to emit code
├── loco-lake/crates/loco-lake/    # DataAdapter trait + adapters (in-memory, SQLite)
└── loco-apps/                     # Axum web server consuming generated types
```

### Dependency Flow

**Build time:** `loco-apps/build.rs` → `loco-gen-codegen-build` → `loco-gen-schema`

**Runtime:** `loco-apps` → `loco-gen-runtime` + `loco-lake`

## How It Works

1. **Define types** in `schemas/types/*.yaml` — each file describes a type with typed properties (string, integer, float, boolean).

2. **Define instances** in `schemas/instances/{user}/{project}/{type}/*.yaml` — these are validated against their type definition at build time.

3. **Build** — `build.rs` parses schemas and instances, generates Rust structs with constructors, accessors, and baked-in instance loaders to `$OUT_DIR/loco_generated.rs`.

4. **Run** — the Axum server loads generated types, exposes a REST API for CRUD operations backed by a pluggable data adapter (SQLite by default).

## Multi-Tenancy

Data is isolated per tenant. Tenants are defined as YAML files in `loco-apps/tenants/`:

```yaml
# tenants/acme.yaml
name: "Acme Corp"
```

The filename (minus `.yaml`) is the tenant ID. Tenant is specified per-request via:

- `X-Tenant-Id` header (for API clients)
- `?tenant=` query parameter (for browser testing)

## Namespaces

Instances are organized into namespaces: `{user}/{project}`. For example:

- `loco/core` — framework-level collections (user, etc.)
- `ben/crm` — application-specific collections (account, contact, opportunity)

## REST API

All data endpoints require a tenant ID.

| Method | Path | Description |
|--------|------|-------------|
| POST | `/{user}/{project}/collection/{name}/add` | Insert a record |
| GET | `/{user}/{project}/collection/{name}/list` | List all records |
| GET | `/{user}/{project}/collection/{name}/get/{id}` | Get a record by ID |
| DELETE | `/{user}/{project}/collection/{name}/delete/{id}` | Delete a record |
| GET | `/meta/{user}/{project}/{type_name}/list` | List type metadata |

### Response Format

```json
{
  "ok": true,
  "data": { ... },
  "error": null
}
```

### Example

```bash
# Insert a user
curl -X POST 'http://localhost:3000/loco/core/collection/user/add?tenant=acme' \
  -H "Content-Type: application/json" \
  -d '{"fields": {"name": "Alice", "email": "alice@acme.com", "role": "admin"}}'

# List users
curl 'http://localhost:3000/loco/core/collection/user/list?tenant=acme'
```

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `LOCO_ADAPTER` | `sqlite` | Data adapter: `sqlite` or `memory` |
| `LOCO_DB_PATH` | `loco.db` | SQLite database file path |

## Rust Edition

2021 — all crates.
