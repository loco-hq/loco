# Loco

Schema-driven backend: YAML type definitions become Rust structs at build time. Instances load at runtime into typed stores. Records live in a schemaless lake and are validated by loco-apps.

Repo: `loco-hq/loco`.

## How agents work here

Two modes. **Direct is the default** — assume you are in it unless Ben has said otherwise in this session.

- **Direct (default).** You and Ben, one session. He directs the work; you implement it, open PRs, file issues, and leave review comments under your own GitHub App identity. Ben reviews and merges. You do not need to read `orchestration.md`, and you do not spawn other agents.
- **Orchestration (opt-in).** Two vendors take turns so no model reviews its own code: an orchestrator picks issues and spawns an implementer and a reviewer via herdr. You are in this mode **only** when Ben puts you in it in so many words — “you’re going to be the orchestrator on this.” Then, and only then, read [`orchestration.md`](orchestration.md) and follow it. Never enter it on your own initiative, and never infer the chair from the repo.

Ben switches modes for his own reasons — often a vendor subscription nearing its usage limit. Do not infer the mode from the state of the repo; infer it from what he asked for.

### Both modes, every session

- **GitHub identity.** Before any `git` or `gh` write: `eval "$(python3 scripts/agent-github/token.py env claude)"` (or `grok` — your vendor). A Claude agent opens PRs, reviews, and comments as `loco-claude[bot]`; a Grok agent as `loco-grok[bot]`. Env vars do not survive between shells, so re-`eval` in each command that writes. Mint the token before `gh pr create`, never after. A PR opened with Ben’s `gh` auth makes Ben the author, and GitHub then blocks him from reviewing his own PR — `main` requires one approving review, admins included. Wrong identity is not cosmetic; fix it by closing and reopening under the app, not by having the wrong actor approve.
- **Never push `main`.** One branch, one PR, every time. Ben merges.
- **Issues are the state.** GitHub issues and milestones are the only backlog — there is no handoff or status file to update. If work is worth remembering, it is worth an issue. Filing rules: [`CONTRIBUTING.md`](CONTRIBUTING.md).
- **Never approve your own PR.** In direct mode Ben is the reviewer; in orchestration mode it is the other vendor.
- PEMs live on this machine at `~/.config/loco-hq/apps/`, not in the repo. If `token.py` fails, stop; do not fall back to Ben’s `gh` auth.

## Commands

```bash
cargo test                    # Workspace tests, including Hurl API suites
cargo test -p loco-gen-schema # Schema/codegen crate only
cargo clippy --workspace      # Lint everything
cargo fmt --all               # Format (CI checks with --check)
cargo run -p loco-apps        # API server on :3000
npm run dev -w loco-studio    # Studio on :5174 (proxies /auth /config /schema /data → :3000)
npm run build -w loco-studio  # Static SPA in loco-studio/dist/ (no Node at runtime)
python3 -m http.server 5174 --directory loco-studio/dist
                              # serve that build; reaches :3000 via API_ORIGIN
python3 -m http.server 5176 --directory examples/public-page
                              # public page on :5176 (CORS → :3000)
npm run dev -w loco-ui        # loco-ui playground on :5175
```

## CI

`.github/workflows/ci.yml` runs on every PR and on pushes to `main`: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace` (which
includes the Hurl suites — the workflow installs the `hurl` binary first). Server-side only; the
frontend workspaces are not built in CI yet.

## Project Structure

```
loco/
├── loco-gen/crates/loco-gen-schema/           # YAML parsing, TypeDef, Rust codegen, build.rs helper
├── loco-gen/crates/loco-gen-schema-fixtures/  # Compiles generated code for both kinds (test-only)
├── loco-schema/crates/loco-schema-runtime/    # InstanceStore + YamlFsAdapter, FileTreeStore + FileTreeFsAdapter
├── loco-lake/crates/loco-lake/                # DataAdapter + InMemoryAdapter + SqliteAdapter
├── loco-apps/                                 # Axum server consuming generated types
├── loco-studio/                               # Schema + record editor
├── loco-ui/                                   # Field component library (npm workspace)
└── examples/public-page/                      # Static cross-origin page (no Node)
```

### Dependency flow

Build-time: `loco-apps/build.rs` → `loco_gen_schema::build::generate`

Runtime: `loco-apps` → generated types + `loco-schema-runtime` + `loco-lake`

There is no `SchemaRegistry`. Generated `SchemaStore` owns one `InstanceStore<T>` per type.

## How Codegen Works

1. `build.rs` calls `loco_gen_schema::build::generate("schemas/types")`
2. Type definitions (`schemas/types/*.yaml`) are parsed into `TypeDef` structs
3. Rust code is generated to `$OUT_DIR/loco_generated.rs` — per-type structs (`new`, accessors, `to_path` / `from_path` / `from_yaml`), an `Update` patch type, a `SchemaInstance` impl, and a `SchemaStore` that constructs one `InstanceStore<T>` per type backed by `YamlFsAdapter`
4. `lib.rs` includes the generated code via `include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"))`

Instances are **not** scanned at build time. At server startup, `SchemaStore::load("schemas/instances")` walks the instances directory, matches each YAML file against its type's `pathTemplate`, and populates the stores.

### Namespace convention

An instance's namespace IS its path relative to `schemas/instances/` with `.yaml` stripped — it matches the type's `pathTemplate` with values filled in. For example:

- `schemas/instances/ben/crm/project.yaml` → `ben/crm/project`
- `schemas/instances/ben/crm/datasets/acme.yaml` → `ben/crm/datasets/acme`
- `schemas/instances/ben/crm/versions/0.0.1/collections/account.yaml` → `ben/crm/versions/0.0.1/collections/account`

`schemas/instances/loco/` is committed (core, studio, cards, demo). Other instance trees (`ben/…`) are gitignored scratch data. Hurl suites use their own fixtures under `loco-apps/tests/suites/*/fixtures/`.

## Schema Files

Type definitions live in `loco-apps/schemas/types/`. Supported field types: `string`, `integer`, `float`, `boolean`, `slug`, and `list` (a `list` requires an `items:` sub-key naming a scalar type, or an inline `object` with `name:` and `properties:` — nested lists are rejected at parse time). Every type has a required `pathTemplate` that controls where instance files live under `schemas/instances/` and how template variables are extracted from file paths. The template is purely logical — it never contains `.yaml`; the storage layer appends that extension when writing to disk.

### Kinds

A type is one of two `kind`s. `kind` is optional and defaults to `document`; any other value is rejected at parse.

- **`document`** (default) — a YAML file at `pathTemplate + ".yaml"`. Everything in `loco-apps/schemas/types/` today.
- **`files`** — a *directory* at `pathTemplate`, no `.yaml`, holding opaque bytes. The files are the instance: codegen emits `to_path` / `from_path` and a `FileTreeStore`, but no `from_yaml`, no `Update` patch, and no field accessors beyond the template variables. A `files` type may declare only its template variables — any other property is a parse error.

Persistence is `FileTreeFsAdapter` (`loco-schema-runtime`). Writes are whole-tree replace and atomic (staged in a sibling temp dir, swapped in with `rename`). Keys and member paths are validated — no `..`, no absolute paths — and symlinks are never followed. A missing tree reads as `None`; it is not a boot failure. `delete_by_prefix` and `copy_by_prefix` on the store see file-tree keys, so a project or version delete cascades and a later copy-version has its copy primitive.

`bundle` is the one production type of this kind — a version's frontend, written by `PUT /schema/{account}/{project}/{version}/bundle` ([`docs/hosting.md`](docs/hosting.md)). `loco-gen-schema-fixtures` is a test-only crate that runs codegen over one type of each kind so the generated code is compiled and exercised by `cargo test`.

### pathTemplate examples

| Type | Template |
|------|----------|
| project | `${project}/project` |
| dataset | `${project}/datasets/${name}` |
| site | `${project}/sites/${name}` |
| bundle | `${project}/versions/${version}/bundle` (a directory — `kind: files`) |
| manifest | `${project}/versions/${version}/manifest` |
| collection | `${project}/versions/${version}/collections/${name}` |
| field | `${project}/versions/${version}/fields/${collection}/${name}` |
| fieldset | `${project}/versions/${version}/fieldsets/${collection}/${name}` |
| permission_set | `${project}/versions/${version}/permission_sets/${name}` |

`${project}` is a multi-segment variable (e.g., `ben/crm`). Hard-coded path segments are always plural (`sites`, `datasets`, `collections`, `fields`, `fieldsets`, `permission_sets`, `versions`).

An inline `object`'s `name:` is snake_case (`collection_grant`); codegen PascalCases it into the generated struct name.

### Versions, sites, datasets

- A **version** is a schema snapshot under `{project}/versions/{version}/`. A version whose name contains `-` is a draft (`0.0.1-dev`); only drafts accept `/schema` writes.
- A **dataset** is a lake partition. Record keys are `(dataset_id, collection, id)` where `dataset_id` is `{user}/{project}/{dataset_name}`.
- A **site** pins a `version` + `dataset`. Requests identify the site with `X-Project-Id: {user}/{project}` and `X-Site-Id: {site}`. There is no tenant header. Token-less `public` may perform any `/data` verb a permission set the **pinned version's manifest** assigns (`public_permission_sets`) grants. Policy is on the version, not the site: two sites pinning one version cannot disagree. Grants are not on the collection. Unspecified verbs default to false.

Creating a project via `/config` bootstraps `0.0.1-dev`, a `dev` dataset, and a `dev` site.

### Manifests and dependency visibility

Each version has a `manifest` instance declaring `dependencies` as `{user}/{project}@{version}` strings and `public_permission_sets` as the names of the permission sets this version assigns to `public`. A consuming version opts into a set a dependency ships by naming it here. `manifest` is a regular schema type — loco-gen treats it no differently than `collection` or `site`.

Dependency grammar and the scoped view live in `loco-apps/src/http/version_schema.rs` (`VersionSchema`). Reads see the version itself plus **direct** dependencies only (not transitive). Writes go to the version's own project, and only when the `VersionSchema` was constructed writable and the version is a draft.

`ProjectConfig` (`http/project_config.rs`) is the same idea for unversioned config: projects, datasets, sites, version create/delete.

#### Name resolution

**Rule: an unqualified name always means _self_ — the project that owns the running
version. A dependency's collection, field, fieldset, or permission set must be named
fully qualified (`{user}/{project}.{name}`) to be reachable.**

The point is that installing a dependency can never silently change what an existing
bare name resolves to. Resolution is a property of the name, not of manifest order.

**This rule is not implemented yet.** Every `VersionSchema` lookup
(`collection`, `field`, `fieldset`, `permission_set`) currently walks self first and
then falls through to direct deps in manifest order, returning the first match. That
means a bare name *can* resolve into a dependency today, and two deps that share a
name make the second unreachable. `collection_grant_matches` (`http/authz.rs`) is the
only place that already accepts the qualified form. Issue #28 tracks making the rule
real across `/data`, `/schema`, and permission-set references. Do not write new code
that relies on the fall-through.

### Fieldsets

A fieldset is an ordered named subset of a collection's fields. `auto_add: true` marks the set that new fields are appended to. `VersionSchema::fields(collection)` returns fields in auto-add fieldset order (then leftover fields alphabetically). Studio uses that order for the collection table; record create/edit forms currently render the same list as returned by `/schema/.../field/{collection}/list`.

## Naming Conventions

These conventions apply to property names in type definitions and variable names in `pathTemplate`s.

- **`id`** — opaque identifier (uuid/number) with no semantic meaning. Immutable once set. Reserved on schema types. Lake records get a UUID `id` stamped by `Record::new_for_insert`.
- **`name`** — semantic slug identifier: `[a-z_]` only, lowercase, immutable. Used as path-segment identifiers in `pathTemplate`s. Declare it as a `slug` + `createOnly` property when it appears in the template; do not repeat it in instance YAML bodies.
- **`label`** — human-readable display string. Any characters, short, mutable.
- **`description`** — free-form text. Any characters, longer, mutable.
- **`project`** — fully-qualified project reference (e.g. `ben/crm`). Used for direct "belongs-to" links and in path templates via `${project}`. Preferred in user-facing contexts.
- **`namespace`** — external scope reference, for pulling inherited metadata from another project. Reserved for cross-project references; do not use as a synonym for `project`.

### Template variables must be declared

Every `${var}` in a `pathTemplate` **must** be declared as a property with `type: slug` and `createOnly: true`. Parse fails otherwise (`TemplateVarNotDeclared` / `TemplateVarNotSlug` / `TemplateVarNotCreateOnly`). At instance-load time the value is extracted from the file path, so instance YAML bodies should not repeat these fields.

## HTTP surface (loco-apps)

Mounted in `server.rs`:

| Prefix | Role |
|--------|------|
| `/data` | Record CRUD. Site-scoped via headers. Strict validation on write; diagnostics on read. |
| `/schema` | Versioned metadata CRUD (manifest, collections, fields, fieldsets, bundle). |
| `/config` | Unversioned project / dataset / site / version lifecycle. |
| `/auth` | Login, logout, `/me` (self), signup (`POST /users`), update/delete (self), API keys. |

CORS is `*` origin, method, and header. No cookies; clients send `Authorization: Bearer`. Studio's Vite proxy is unchanged.

Handlers sit on request extractors in `http/scope/`:

- `SiteScope` — resolves project + site from headers, attaches auth (or `public`), builds a **read-only** `VersionSchema` for the site's pinned version. Home of `require_authenticated`, `require_developer`, `require_can_write_data`. Access is membership, not the site.
- `VersionScope` — authenticated identity plus a **writable** `VersionSchema` for the path triple. Requires developer (or org owner) on the path project. Used by `/schema` writes.
- `VersionReadScope` — read-only `VersionSchema` for GET `/schema`. Developer/editor on the path project (any version, no site headers). `public` (and authenticated non-members) on a site whose pinned version assigns at least one permission set to `public` (pinned version only; `X-Project-Id` + `X-Site-Id` required).
- `ConfigProjectScope` / `ConfigUserScope` — `/config` routes. Project-targeted routes require developer; list/create/org do not need site headers.
- `CollectionScope` / `RecordScope` — `/data` routes. Authenticated writes need editor or developer. Token-less `public` may list/get/insert/update/delete when a permission set the pinned version's manifest assigns to `public` grants that verb on that collection.

Membership: `org_members (org, identity, owner|member)` and `project_members (project, identity, developer|editor)`. Effective project access = org owner ∪ project role, plus implicit developer when the identity owns the person account (`alice` → `alice/*`).

Login (`POST /auth/login`) is global — it does not use `X-Site-Id` to find the user. Body is `{ "username", "password" }`. Seeded identities (`alice`, `bob`) have password `password`. Org accounts (`loco`) cannot log in. Sessions and API keys hang off the identity and both work as `Authorization: Bearer …`. `GET /auth/me` is authenticated self-read. `POST /auth/users` is self-service signup (no token; password required). `PUT` / `DELETE /auth/users/{id}` are self only — org owners manage membership, not the person account. There is no `/auth/users/list`. Login auto-creates an unknown handle only when `LOCO_AUTH_AUTO_CREATE=1` (Hurl) or `cfg(test)`. Credentials are hashed at rest by `auth/secret.rs`: passwords with argon2id, API keys with SHA-256 (the plaintext key is returned once, from `POST /auth/api-keys`). Local `auth/*.json` files written before hashing are re-hashed in place on load. Sessions expire `SESSION_TTL_DAYS` (7) after login — there is no refresh, so the client logs in again. `validate_session` returns `SessionExpired` past that point and drops the session from the cache and from disk; expired sessions left on disk are swept at startup. API keys do not expire; they are revoked.

### Validation

Lives in `loco-apps/src/validation.rs`, not in the lake. Checks unknown fields and scalar type mismatches (`string` / `integer` / `float` / `boolean`). `Null` is allowed for any type. Declared types the validator does not know (including `list`) pass. There is no `required` flag on `Field` yet.

## Key Patterns

- **Rust keyword escaping**: Codegen emits `r#type` (etc.) for property names that are Rust keywords. See `rust_ident()` in `codegen.rs`.
- **Error types**: Each crate has its own error enum — `loco_gen_schema::Error`, `loco_schema_runtime::Error`, `loco_lake::Error`.
- **Tests**: Unit tests are co-located (`#[cfg(test)] mod tests`). Filesystem tests use `tempfile`. API tests are Hurl suites under `loco-apps/tests/suites/`, driven by `tests/hurl_runner.rs`.
- **Thread safety**: `InstanceStore` uses `RwLock<BTreeMap<...>>`. `InMemoryAdapter` uses `RwLock<HashMap<...>>`. `SqliteAdapter` uses `Mutex<Connection>`.

## Frontend Apps

All frontend apps use the same stack:

- **Vite** with `@vitejs/plugin-react`
- **React** (functional components, hooks)
- **React Router** (`react-router-dom`, `createHashRouter`)
- **TanStack Query**
- API client in `src/api.js` (plain JS, not a hook). Auth helpers in `src/auth.js`.
- Components in `src/components/` as `.jsx` files
- Dev-only Vite proxy of `/auth` `/config` `/schema` `/data` to `localhost:3000` (no `/api` prefix; production bundle uses `API_ORIGIN` in `loco-studio/src/config.js`)

### Frontend locations

- `loco-studio/` — Schema + record UI (port 5174). The token is the person. Schema/config calls do not need site headers. Data calls send `X-Project-Id` / `X-Site-Id` for the browsed site.
- `loco-ui/` — Reusable field library (no library build; consumed via npm workspaces). Playground at port 5175 (`npm run dev -w loco-ui`).

### loco-ui

Field components for rendering schema metadata. Two layers:

- **Primitives** — `TextField`, `NumberField`, `CheckboxField`, `ToggleField`, `SelectField`. Uniform shell props: `id`, `label`, `description`, `error`, `required`, `disabled`, `value`, `onChange`, plus type-specific props.
- **Dispatcher** — `<Field field={meta} variant?="..." />` picks a primitive from a hardcoded `type → variant → component` registry. `variant` can come from field metadata or be overridden at the call site. `SelectField` is exported but not registered in the dispatcher.

Styling is plain CSS via `.module.css` files co-located with each component. Shared design tokens (`--loco-*` CSS variables) live in `src/_shell/tokens.css` and must be imported once by the consumer (`import 'loco-ui/tokens.css'`).

Collection field metadata (`field.yaml`) currently has only `type` and `label`. The dispatcher already reads `description`, `required`, and `variant` when present.

## Rust Edition

2021 — all crates.
