# Loco

Schema-driven backend for structured data. YAML type definitions become Rust structs at build time. Projects, collections, and fields are versioned YAML instances loaded at runtime. Records live in a schemaless data lake and are validated against the site's schema: **strict on write, warnings on read**.

Studio (`loco-studio`) is a React app for editing schemas and records. It is itself a Loco site.

## Quick Start

```bash
# API server — http://localhost:3000
cargo run -p loco-apps

# Studio — http://localhost:5174  (proxies /api to :3000)
npm run dev -w loco-studio

# Field-component playground — http://localhost:5175
npm run dev -w loco-ui

# Tests / lint
cargo test
cargo clippy --workspace
```

Studio login is username-only right now. The local auth adapter stores users and sessions under `loco-apps/auth/` (gitignored).

## Architecture

```
loco/
├── loco-gen/crates/loco-gen-schema/           # YAML type parsing + Rust codegen
├── loco-schema/crates/loco-schema-runtime/    # InstanceStore + YAML filesystem adapter
├── loco-lake/crates/loco-lake/                # DataAdapter (SQLite, in-memory)
├── loco-apps/                                 # Axum server: /data /schema /config /auth
├── loco-studio/                               # Schema + record editor (port 5174)
└── loco-ui/                                   # Field primitives consumed by studio
```

**Build time:** `loco-apps/build.rs` → `loco_gen_schema::build::generate("schemas/types")`

**Runtime:** `SchemaStore::load("schemas/instances")`, then Axum with a lake adapter and an auth adapter.

There is no tenant registry. Isolation is **dataset** (where records live) plus **site** (which version and dataset a request is pinned to).

## Core concepts

| Concept | What it is |
|---------|------------|
| **Project** | A namespace, `{user}/{project}` (e.g. `ben/pets`, `loco/studio`) |
| **Version** | A snapshot of that project's schema. Drafts have a `-` in the name (`0.0.1-dev`); only drafts are writable |
| **Manifest** | Per-version file listing direct dependencies (`{user}/{project}@{version}`) |
| **Collection / field / fieldset** | Schema for a kind of record, its columns, and named ordered subsets of those columns |
| **Dataset** | A lake partition. Records are keyed `(dataset_id, collection, id)` |
| **Site** | An app identity. Pins a `version` + `dataset`. Requests name it with `X-Project-Id` + `X-Site-Id` |

```
ben/pets
├── project.yaml
├── datasets/dev.yaml          # lake partition
├── sites/dev.yaml             # pins version 0.0.1-dev + dataset dev
└── versions/0.0.1-dev/
    ├── manifest.yaml
    ├── collections/pet.yaml
    ├── fields/pet/{name,age,breed}.yaml
    └── fieldsets/pet/default.yaml
```

Shipped projects (committed under `schemas/instances/loco/`):

- `loco/core` — framework collections (`user`)
- `loco/studio` — the editor site (`studio`)
- `loco/cards` — another metadata-editor site

User-scoped instances (`ben/…`) are gitignored scratch data. Test suites carry their own fixtures.

## How schema loading works

1. Type definitions in `loco-apps/schemas/types/*.yaml` are parsed at **build** time.
2. Codegen writes `$OUT_DIR/loco_generated.rs` — one struct per type, plus `SchemaStore`.
3. `main.rs` includes that file. Instances are **not** compiled in.
4. On boot, `SchemaStore::load` walks `schemas/instances/`, matches each YAML file against the type's `pathTemplate`, and fills per-type `InstanceStore`s. Writes go back to disk via `YamlFsAdapter`.

An instance's key **is** its path relative to `schemas/instances/` with `.yaml` stripped. That key must match the type's `pathTemplate` with variables filled in.

## REST API

The server listens on `:3000`. Studio rewrites `/api/…` to these paths.

Every request that needs a site sends:

- `X-Project-Id: {user}/{project}`
- `X-Site-Id: {site}`
- `Authorization: Bearer <token>` when authenticated

Missing auth becomes a synthetic `public` user. Writes that require a real session return `401`. `/schema` and `/config` writes also require the site to be on the metadata-editor allowlist (`loco/studio/studio`, `loco/cards/cards`) and the session user to match the `{user}` in the path.

### `/data` — records (scoped by the site's dataset + version)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/data/{collection}/add` | Insert. Body is the field map. Validated strictly. |
| GET | `/data/{collection}/list` | List. Schema drift comes back as `diagnostics` warnings. |
| GET | `/data/{collection}/get/{id}` | Get one |
| PUT | `/data/{collection}/update/{id}` | Patch fields. Validated strictly. |
| DELETE | `/data/{collection}/delete/{id}` | Delete |

Studio overrides `X-Project-Id` / `X-Site-Id` on these calls so it can edit records in the site you are browsing, not only `loco/studio`.

### `/schema` — versioned metadata (path is `{user}/{project}/{version}`)

Collections, fields, fieldsets, and the version manifest. Writable only on draft versions.

### `/config` — unversioned project config

Projects, datasets, sites, and version create/list/delete. Creating a project bootstraps `0.0.1-dev`, a `dev` dataset, and a `dev` site.

### `/auth`

Login (username only), logout, `/me`, users, API keys. Sessions and keys are stored by the local filesystem auth adapter under `loco-apps/auth/{user}/{project}/{site}/`.

### Response shape

```json
{
  "ok": true,
  "data": { },
  "error": null,
  "diagnostics": null
}
```

`diagnostics` is present on reads that found schema drift, and on failed writes (`400`, `error: "validation failed"`).

### Example

```bash
# List pets in ben/pets's dev site
curl 'http://localhost:3000/data/pet/list' \
  -H 'X-Project-Id: ben/pets' \
  -H 'X-Site-Id: dev' \
  -H 'Authorization: Bearer <token>'

# Insert
curl -X POST 'http://localhost:3000/data/pet/add' \
  -H 'X-Project-Id: ben/pets' -H 'X-Site-Id: dev' \
  -H 'Authorization: Bearer <token>' \
  -H 'Content-Type: application/json' \
  -d '{"name": "Mochi", "age": 3, "breed": "mutt"}'
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `LOCO_ADAPTER` | `sqlite` | Data adapter: `sqlite` or `memory` |
| `LOCO_DB_PATH` | `loco.db` | SQLite file (created next to the process cwd) |
| `LOCO_AUTH_ADAPTER` | `local` | Auth adapter. Only `local` exists. |

## Frontends

- **loco-studio** (5174) — project / version / collection / field / record UI. API client is `src/api.js`; session token in `localStorage`.
- **loco-ui** (5175 playground) — field primitives (`TextField`, `NumberField`, `CheckboxField`, `ToggleField`, `SelectField`) plus a `<Field field={meta} />` dispatcher. Consumed by studio as an npm workspace package.

## Tests

Workspace unit tests live next to their modules. API coverage is Hurl suites under `loco-apps/tests/suites/` (authorization, data CRUD, validation reads/writes, project/version lifecycle, schema CRUD + introspect). `cargo test -p loco-apps` spins up a server per suite against that suite's fixtures.

## Rust Edition

2021 — all crates.
