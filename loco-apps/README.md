# loco-apps

Axum server that exposes generated schema types and a schemaless record lake over HTTP.

## Running

```bash
cargo run -p loco-apps
```

Listens on `http://localhost:3000`. Studio in dev (`:5174`) proxies `/auth` `/config` `/schema` `/data` here; a production build of Studio is a static SPA that calls these paths over CORS (or same-origin once Axum serves `dist/`). A static page on another origin (`examples/public-page/`, typically `:5176`) talks to this server directly; CORS is `*` origin, method, and header (no cookies).

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `LOCO_ADAPTER` | `sqlite` | `sqlite` or `memory` |
| `LOCO_DB_PATH` | `loco.db` | SQLite file path |
| `LOCO_AUTH_ADAPTER` | `local` | Only `local` exists |
| `LOCO_AUTH_AUTO_CREATE` | unset | Login of an unknown handle creates a person (`1`/`true`; Hurl sets this) |

## How it boots

1. `build.rs` generates Rust types from `schemas/types/` via `loco_gen_schema::build::generate`
2. `lib.rs` includes `$OUT_DIR/loco_generated.rs`
3. `server::build_app()` loads instances from `schemas/instances/` into `SchemaStore`
4. A lake adapter (`sqlite` / `memory`) and the local auth adapter (`auth/`) are constructed
5. Routes are nested: `/data`, `/schema`, `/config`, `/auth`

Instances are not compiled in. Changing a type definition requires a rebuild; changing an instance YAML is picked up on the next process start (or immediately, if the write went through the API).

## Request identity

There is no tenant header. A request names a **site**:

- `X-Project-Id: {user}/{project}`
- `X-Site-Id: {site}`
- `Authorization: Bearer <session or api key>` (optional)

`SiteScope` resolves the site, pins the lake to that site's dataset, and builds a read-only `VersionSchema` for the site's version. Missing auth becomes the synthetic `public` user. Anonymous `/data` CRUD is the union of permission sets the **pinned version's manifest** assigns to `public` (`public_permission_sets`); each verb is whatever those sets grant. The assignment is on the version, not the site, so two sites pinning one version cannot disagree about it.

GET `/schema` is allowed for a project `developer` or `editor` (any version, no site headers), and for `public` on a site whose pinned version assigns at least one permission set to `public` (pinned version only; `X-Project-Id` + `X-Site-Id` required). Authenticated non-members may use that public read.

`/schema` writes and `/config` additionally require:

- a real session
- project `developer` (or org owner) on the path project
- a draft version (name contains `-`) for `/schema` mutations

## Routes

### `/data/{collection}/…`

Record CRUD against the site's dataset. Body for add/update is a JSON field map (not wrapped). Writes run `validation.rs` in create/update mode and reject errors. Reads attach diagnostics as warnings when stored data has drifted from the schema.

Lake collection keys are `{user}/{project}.{name}`.

### `/schema/{user}/{project}/{version}/…`

Versioned metadata: manifest, collections, fields, fieldsets, permission sets. GET goes through `VersionReadScope`; writes through `VersionScope` → `VersionSchema`.

### `/config/…`

Unversioned config: projects, datasets, sites, version create/list/delete. Creating a project bootstraps `0.0.1-dev` + `dev` dataset + `dev` site. Deleting a project cascades schema files and returns dataset names so the lake can be purged.

### `/auth/…`

`POST /login` is global (`{ "username" }`, optional `"password"`; no site headers required). `POST /logout`, `GET /me` (authenticated self-read), `POST /users` (self-service signup, password required), `PUT`/`DELETE /users/{id}` (self). Persistence is `auth/{accounts,identities,sessions,api_keys}/` (gitignored). Login auto-creates unknown handles only when `LOCO_AUTH_AUTO_CREATE` is set.

## Schemas on disk

```
schemas/
├── types/                 # Type definitions (rebuild to change)
└── instances/
    └── loco/              # Committed: core, studio, cards, demo
        ├── core/
        ├── studio/
        ├── cards/
        └── demo/          # public guestbook (`www`)
```

`schemas/instances/*` other than `loco/` is gitignored. Hurl fixtures live under `tests/suites/*/fixtures/`.

## Tests

```bash
cargo test -p loco-apps
```

`tests/hurl_runner.rs` builds the app against a tempdir (real `schemas/types/` + the suite's `fixtures/`) and runs every `.hurl` file in that suite.
