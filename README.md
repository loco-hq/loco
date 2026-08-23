# Loco

Schema-driven backend for structured data. YAML type definitions become Rust structs at build time. Projects, collections, and fields are versioned YAML instances loaded at runtime. Records live in a schemaless data lake and are validated against the site's schema: **strict on write, warnings on read**.

Studio (`loco-studio`) is a React app for editing schemas and records. It is itself a Loco site.

## Quick Start

```bash
# API server — http://localhost:3000
cargo run -p loco-apps

# Studio (dev) — http://localhost:5174  (proxies /auth /config /schema /data → :3000)
npm run dev -w loco-studio

# Studio (production build, no Node) — http://localhost:5174
npm run build -w loco-studio
python3 -m http.server 5174 --directory loco-studio/dist

# Cross-origin public page — http://localhost:5176  (talks to :3000, CORS)
python3 -m http.server 5176 --directory examples/public-page

# Field-component playground — http://localhost:5175
npm run dev -w loco-ui

# Tests / lint
cargo test
cargo clippy --workspace
```

Studio login is a global Loco identity (username, optional password). The local auth adapter stores accounts, identities, sessions, and API keys under `loco-apps/auth/` (gitignored) — not under a site path.

## Architecture

```
loco/
├── loco-gen/crates/loco-gen-schema/           # YAML type parsing + Rust codegen
├── loco-schema/crates/loco-schema-runtime/    # InstanceStore + YAML filesystem adapter
├── loco-lake/crates/loco-lake/                # DataAdapter (SQLite, in-memory)
├── loco-apps/                                 # Axum server: /data /schema /config /auth
├── loco-studio/                               # Schema + record editor (port 5174)
├── loco-ui/                                   # Field primitives consumed by studio
└── examples/public-page/                      # Static cross-origin page (port 5176)
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
- `loco/demo` — public guestbook site (`www`) for the standalone frontend example

User-scoped instances (`ben/…`) are gitignored scratch data. Test suites carry their own fixtures.

## How schema loading works

1. Type definitions in `loco-apps/schemas/types/*.yaml` are parsed at **build** time.
2. Codegen writes `$OUT_DIR/loco_generated.rs` — one struct per type, plus `SchemaStore`.
3. `main.rs` includes that file. Instances are **not** compiled in.
4. On boot, `SchemaStore::load` walks `schemas/instances/`, matches each YAML file against the type's `pathTemplate`, and fills per-type `InstanceStore`s. Writes go back to disk via `YamlFsAdapter`.

An instance's key **is** its path relative to `schemas/instances/` with `.yaml` stripped. That key must match the type's `pathTemplate` with variables filled in.

## REST API

The server listens on `:3000`. Browser clients call `/auth`, `/config`, `/schema`, and `/data` directly. Studio is a static SPA: `npm run build -w loco-studio` emits `loco-studio/dist/` (HTML/JS/CSS). Runtime has no Node; producing `dist/` still needs `npm run build`. The API origin is `API_ORIGIN` in `loco-studio/src/config.js` (default `http://localhost:3000`). Dev (`npm run dev -w loco-studio`) keeps a Vite proxy of those four prefixes for HMR; the same constant is used so the built SPA talks to `:3000` over CORS.

Browser clients on another origin are allowed: CORS is `*` origin, method, and header. Sessions are `Authorization: Bearer`, not cookies, so `*` is legal. `examples/public-page/` is a static page that lists `loco/demo` guestbook with `{ apiUrl, projectId, siteId }` and no token.

Every request that needs a site sends:

- `X-Project-Id: {user}/{project}`
- `X-Site-Id: {site}`
- `Authorization: Bearer <token>` when authenticated

Missing auth becomes a synthetic `public` user. Anonymous `/data` CRUD is the union of permission sets the site assigns to `public`; unspecified verbs default to false. GET `/schema` is allowed for project `developer` / `editor`, and for `public` on a site that assigns at least one permission set (pinned version, site headers required). `/schema` writes and `/config` require a real session and project `developer` (or org owner).

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

Collections, fields, fieldsets, permission sets, and the version manifest. GET: developer/editor, or `public` on a site with at least one assigned permission set (pinned version). Writable only on draft versions (developer).

### `/config` — unversioned project config

Projects, datasets, sites, and version create/list/delete. Creating a project bootstraps `0.0.1-dev`, a `dev` dataset, and a `dev` site.

### `/auth`

Login (global identity; `{ "username" }` plus optional `"password"`), logout, `GET /me` (any authenticated identity), users (org owner of at least one org), API keys. Sessions and keys hang off the identity and are stored under `loco-apps/auth/{accounts,identities,sessions,api_keys}/`.

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

- **loco-studio** (5174 in dev) — project / version / collection / field / record UI. Static production build in `dist/`. API client is `src/api.js`; session token in `localStorage`. API origin is `API_ORIGIN` in `src/config.js`.
- **loco-ui** (5175 playground) — field primitives (`TextField`, `NumberField`, `CheckboxField`, `ToggleField`, `SelectField`) plus a `<Field field={meta} />` dispatcher. Consumed by studio as an npm workspace package.

## Tests

Workspace unit tests live next to their modules. API coverage is Hurl suites under `loco-apps/tests/suites/` (authorization, data CRUD, validation reads/writes, project/version lifecycle, schema CRUD + introspect). `cargo test -p loco-apps` spins up a server per suite against that suite's fixtures.

## Rust Edition

2021 — all crates.
