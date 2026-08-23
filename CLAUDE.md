# Loco

Schema-driven backend: YAML type definitions become Rust structs at build time. Instances load at runtime into typed stores. Records live in a schemaless lake and are validated by loco-apps.

## Commands

```bash
cargo test                    # Workspace tests, including Hurl API suites
cargo test -p loco-gen-schema # Schema/codegen crate only
cargo clippy --workspace      # Lint everything
cargo run -p loco-apps        # API server on :3000
npm run dev -w loco-studio    # Studio on :5174 (proxies /auth /config /schema /data → :3000)
npm run build -w loco-studio  # Static SPA in loco-studio/dist/ (no Node at runtime)
python3 -m http.server 5176 --directory examples/public-page
                              # public page on :5176 (CORS → :3000)
npm run dev -w loco-ui        # loco-ui playground on :5175
```

## Project Structure

```
loco/
├── loco-gen/crates/loco-gen-schema/           # YAML parsing, TypeDef, Rust codegen, build.rs helper
├── loco-schema/crates/loco-schema-runtime/    # SchemaInstance, InstanceStore, YamlFsAdapter
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

### pathTemplate examples

| Type | Template |
|------|----------|
| project | `${project}/project` |
| dataset | `${project}/datasets/${name}` |
| site | `${project}/sites/${name}` |
| manifest | `${project}/versions/${version}/manifest` |
| collection | `${project}/versions/${version}/collections/${name}` |
| field | `${project}/versions/${version}/fields/${collection}/${name}` |
| fieldset | `${project}/versions/${version}/fieldsets/${collection}/${name}` |
| permission_set | `${project}/versions/${version}/permission_sets/${name}` |

`${project}` is a multi-segment variable (e.g., `ben/crm`). Hard-coded path segments are always plural (`sites`, `datasets`, `collections`, `fields`, `fieldsets`, `permission_sets`, `versions`).

### Versions, sites, datasets

- A **version** is a schema snapshot under `{project}/versions/{version}/`. A version whose name contains `-` is a draft (`0.0.1-dev`); only drafts accept `/schema` writes.
- A **dataset** is a lake partition. Record keys are `(dataset_id, collection, id)` where `dataset_id` is `{user}/{project}/{dataset_name}`.
- A **site** pins a `version` + `dataset`. Requests identify the site with `X-Project-Id: {user}/{project}` and `X-Site-Id: {site}`. There is no tenant header. Token-less `public` may perform any `/data` verb a permission set the site assigns (`public_permission_sets`) grants. Grants are not on the collection. Unspecified verbs default to false.

Creating a project via `/config` bootstraps `0.0.1-dev`, a `dev` dataset, and a `dev` site.

### Manifests and dependency visibility

Each version has a `manifest` instance declaring `dependencies` as `{user}/{project}@{version}` strings. `manifest` is a regular schema type — loco-gen treats it no differently than `collection` or `site`.

Dependency grammar and the scoped view live in `loco-apps/src/http/version_schema.rs` (`VersionSchema`). Reads see the version itself plus **direct** dependencies only (not transitive). Writes go to the version's own project, and only when the `VersionSchema` was constructed writable and the version is a draft.

`ProjectConfig` (`http/project_config.rs`) is the same idea for unversioned config: projects, datasets, sites, version create/delete.

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
| `/schema` | Versioned metadata CRUD (manifest, collections, fields, fieldsets). |
| `/config` | Unversioned project / dataset / site / version lifecycle. |
| `/auth` | Login, logout, `/me` (self), users (org owner), API keys. |

CORS is `*` origin, method, and header. No cookies; clients send `Authorization: Bearer`. Studio's Vite proxy is unchanged.

Handlers sit on request extractors in `http/scope/`:

- `SiteScope` — resolves project + site from headers, attaches auth (or `public`), builds a **read-only** `VersionSchema` for the site's pinned version. Home of `require_authenticated`, `require_developer`, `require_can_write_data`. Access is membership, not the site.
- `VersionScope` — authenticated identity plus a **writable** `VersionSchema` for the path triple. Requires developer (or org owner) on the path project. Used by `/schema` writes.
- `VersionReadScope` — read-only `VersionSchema` for GET `/schema`. Developer/editor on the path project (any version, no site headers). `public` (and authenticated non-members) on a site that assigns at least one permission set to `public` (pinned version only; `X-Project-Id` + `X-Site-Id` required).
- `ConfigProjectScope` / `ConfigUserScope` — `/config` routes. Project-targeted routes require developer; list/create/org do not need site headers.
- `CollectionScope` / `RecordScope` — `/data` routes. Authenticated writes need editor or developer. Token-less `public` may list/get/insert/update/delete when a permission set the site assigns to `public` grants that verb on that collection.

Membership: `org_members (org, identity, owner|member)` and `project_members (project, identity, developer|editor)`. Effective project access = org owner ∪ project role, plus implicit developer when the identity owns the person account (`alice` → `alice/*`).

Login (`POST /auth/login`) is global — it does not use `X-Site-Id` to find the user. Body is `{ "username", "password" }`. Seeded identities (`alice`, `bob`) have password `password`. Org accounts (`loco`) cannot log in. Sessions and API keys hang off the identity and both work as `Authorization: Bearer …`. `GET /auth/me` is authenticated self-read. `/auth/users` (list/create/update/delete) requires the caller to be `owner` of at least one org; person-account ownership does not qualify.

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
