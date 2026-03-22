# loco-apps

Axum web server that serves generated types via a multi-tenant REST API backed by loco-lake.

## Running

```bash
cd loco-apps && cargo run
```

The server listens on `http://localhost:3000`.

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `LOCO_ADAPTER` | `sqlite` | `sqlite` or `memory` |
| `LOCO_DB_PATH` | `loco.db` | Path to SQLite database file |

## Tenants

Each tenant is a YAML file in `tenants/`:

```yaml
# tenants/acme.yaml
name: "Acme Corp"
```

The filename (without `.yaml`) is the tenant ID used in requests.

Tenant is resolved from requests in this order:
1. `X-Tenant-Id` header
2. `?tenant=` query parameter

Requests without a valid tenant receive a `400 Bad Request`.

## Schemas

Type definitions and instances live in `schemas/`:

```
schemas/
├── types/              # Type definitions (collection.yaml, field.yaml)
└── instances/          # Instance data organized by namespace
    ├── loco/core/      # Framework collections (user)
    └── ben/crm/        # App-specific collections (account, contact, opportunity)
```

Adding or modifying schema files requires a rebuild (`cargo build`) since codegen runs at build time via `build.rs`.

## API Endpoints

All data endpoints require a tenant ID.

```bash
# Insert a record
curl -X POST 'http://localhost:3000/ben/crm/collection/account/add?tenant=acme' \
  -H "Content-Type: application/json" \
  -d '{"fields": {"company": "Acme Corp", "active": true}, "owner": "alice"}'

# List records
curl 'http://localhost:3000/ben/crm/collection/account/list?tenant=acme'

# Get a record
curl 'http://localhost:3000/ben/crm/collection/account/get/{id}?tenant=acme'

# Delete a record
curl -X DELETE 'http://localhost:3000/ben/crm/collection/account/delete/{id}?tenant=acme'

# List type metadata
curl 'http://localhost:3000/meta/ben/crm/collection/list'
```

## How It Boots

1. `build.rs` generates Rust code from `schemas/` via `loco-gen-codegen-build`
2. `main.rs` includes the generated code and calls `server::build_app()`
3. `build_app()` loads tenants from `tenants/*.yaml`
4. Generated `Collection::load_all_instances()` and `Field::load_all_instances()` provide metadata
5. The selected data adapter (SQLite or in-memory) is initialized
6. Axum routes are wired up with shared `AppState`
