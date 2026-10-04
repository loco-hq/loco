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
                              # PORT overrides the port (default 3000).
                              # LOCO_ROOT overrides the data directory
                              # (default loco-apps/): schemas/ and auth/.
                              # Unset, the SQLite file stays LOCO_DB_PATH
                              # or ./loco.db in the working directory — the
                              # repo-root dev database. Set, a relative
                              # database path is resolved under that root.
                              # An absolute LOCO_DB_PATH is used as given.
npm run dev -w loco-studio    # Studio on :5174 (proxies /auth /config /schema /data /actions → :3000)
npm run build -w loco-studio  # Static SPA in loco-studio/dist/ (no Node at runtime)
                              # to serve it, zip dist/ and PUT it to a draft's
                              # bundle, then point a site at that version
                              # (docs/hosting.md). API_ORIGIN is '' — the built
                              # SPA talks to whatever origin serves it.
python3 -m http.server 5176 --directory examples/public-page
                              # public page on :5176 (CORS → :3000)
npm run dev -w loco-ui        # loco-ui playground on :5175
npm run deploy --prefix examples/brickos-inventory
                              # build + PUT the BrickOS batch editor as the
                              # brickos/inventory@0.0.1-dev bundle; needs
                              # LOCO_USER / LOCO_PASSWORD (its README). A
                              # root `npm install` covers it: it is a
                              # workspace, like loco-client
npm test -w loco-client       # loco-client unit tests (node --test)
```

## CI

`.github/workflows/ci.yml` runs on every PR and on pushes to `main`: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace` (which
includes the Hurl suites — the workflow installs the `hurl` binary first). A second, independent
job runs the frontend workspaces on Node 24: `npm ci` at the root, `npm test -w loco-client`, then
`npm run build --workspaces --if-present` (loco-studio, loco-ui, brickos-inventory).

## Project Structure

```
loco/
├── loco-gen/crates/loco-gen-schema/           # YAML parsing, TypeDef, Rust codegen, build.rs helper
├── loco-gen/crates/loco-gen-schema-fixtures/  # Compiles generated code for both kinds (test-only)
├── loco-schema/crates/loco-schema-runtime/    # InstanceStore + YamlFsAdapter, FileTreeStore + FileTreeFsAdapter
├── loco-lake/crates/loco-lake/                # DataAdapter + InMemoryAdapter + SqliteAdapter
├── loco-apps/                                 # Axum server consuming generated types
│   └── schemas/{types,seed,instances}/       # type defs, committed seed, live schema store (gitignored)
├── loco-studio/                               # Schema + record editor
├── loco-ui/                                   # Field component library (npm workspace)
├── loco-client/                               # Plain-JS API client (npm workspace)
├── examples/public-page/                      # Static cross-origin page (no Node)
└── examples/brickos-inventory/                # Hosted Vite app: BrickOS batch editor
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

Instances are **not** scanned at build time. At server startup, `seed::seed_instances` first copies each committed project in `schemas/seed/` into `schemas/instances/` if the store lacks it, then `SchemaStore::load("schemas/instances")` walks the instances directory, matches each YAML file against its type's `pathTemplate`, and populates the stores.

`YamlFsAdapter` writes each file atomically: temp sibling, fsync, `rename` over the target, fsync the directory. A crash leaves the old file, never a truncated one. If only the directory fsync fails, the new file is already in place: the write returns `Error::NotDurable` and the store still updates its cache, so cache and disk agree. Temp artefacts are named `.loco-*` and `load_all` skips them, so a leftover never blocks boot. A YAML file that does not parse still fails boot, on purpose — for a local server that is the honest answer.

### Namespace convention

An instance's namespace IS its path relative to `schemas/instances/` with `.yaml` stripped — it matches the type's `pathTemplate` with values filled in. For example:

- `schemas/instances/ben/crm/project.yaml` → `ben/crm/project`
- `schemas/instances/ben/crm/datasets/acme.yaml` → `ben/crm/datasets/acme`
- `schemas/instances/ben/crm/versions/0.0.1/collections/account.yaml` → `ben/crm/versions/0.0.1/collections/account`

`schemas/instances/` is the live schema store and is wholly gitignored — every `/schema` write and every `/config` write of a project, dataset, site, or version, bundle deploys included, lands there and never in a tracked file. Secret and variable *values* are not schema files: they are lake records (below). Committed projects live in `schemas/seed/` with the same layout: `seed/loco/` (core, studio, demo) and `seed/brickos/` (inventory, [`docs/brickos.md`](docs/brickos.md)). At boot (`loco-apps/src/seed.rs`) a seed project is copied whole into the store only when the store has no `{account}/{project}/project.yaml` for it — decided by that file, not the directory, so leftovers such as an old `bundle/` do not block it. Files already in the store are never overwritten, and a seed is never re-synced: to pick up a changed seed, delete the project from `schemas/instances/` and restart. The server never writes to `schemas/seed/`; change it by editing the YAML and committing. Scratch projects (`ben/…`) live only in the store.

A project whose account does not exist loads anyway and logs one warning at boot. The `brickos` org account is not created at boot: create it with `POST /config/org` and its creator owns it. Hurl suites use their own fixtures under `loco-apps/tests/suites/*/fixtures/`, and a root with no `schemas/seed/` seeds nothing; `run_suite_over_seed` in `tests/hurl_runner.rs` copies named seed accounts in for suites that need them.

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
| secret | `${project}/versions/${version}/secrets/${name}` |
| variable | `${project}/versions/${version}/variables/${name}` |
| action | `${project}/versions/${version}/actions/${name}` |
| integration_type | `${project}/versions/${version}/integration_types/${name}` |
| integration | `${project}/versions/${version}/integrations/${name}` |

`${project}` is a multi-segment variable (e.g., `ben/crm`). Hard-coded path segments are always plural (`sites`, `datasets`, `collections`, `fields`, `fieldsets`, `permission_sets`, `secrets`, `variables`, `actions`, `integration_types`, `integrations`, `versions`).

An `action` carries its input as `params`, an inline list of `action_param` objects (`name`, `type`, `label`, `description`, `required`, `options`). They are written with the action: a PUT that names `params` replaces the list, and there is no param route. On create and on that replacement, each `name` must be a slug (`[a-z0-9_.-]+`, one segment) and unique within the action, each `type` must pass the `FIELD_TYPES` check, and `options` are only meaningful for `string` — anything else is a 400 and nothing is stored. A consumer cannot add a param to a dependency's action, because the list lives on the owner's document.

An integration type and an integration are separate schema types from ordinary collections, fields, and actions. The model is [`docs/integrations.md`](docs/integrations.md). A type at `integration_types/${name}` carries inline lists: `integration_secret` and `integration_variable` (a secret has no `default` and no `value`, a variable may set `default`, empty means none, and a shared name across the two lists is a 400), `collections` of `integration_collection` (each with `fields` of `integration_field`, and `options` of `integration_field_option`), and `actions` of `integration_action` (each with `params` of `integration_action_param`, the same shape as an ordinary action param). An integration at `integrations/${name}` names a type, bare or `{account}/{project}.{name}`, and carries custom collections in the same `integration_collection` list. A PUT that names a list replaces it. On create and on that replacement, names are slugs and unique within their list, field and param types pass `FIELD_TYPES`, and `options` are only meaningful for `string`. A custom collection may not reuse a name its type offers, and a type's collections list may not add a name an integration of that type in the same version already declares as custom. Either write is 400 naming both documents. A manifest dependency, including one a version copy carries, is refused the same way when a dependency type offers a custom name. A consumer cannot add to a dependency's type or integration: the path name is this version's document. Deleting an integration removes the lists on it. Deleting a type an integration still names is 409 naming every such integration — one in this version whose canonical `type` is the bare name, and one in another project's version whose manifest depends on this version and whose `type` is `{this project}.{name}`. The check and the delete run under `PINS`. Version and project delete still cascade both documents. Version copy carries both documents. Addressing `{integration}:{name}` is resolved on `/data`, `/actions`, `/data/query`, and grants. A data verb on an unambiguous integration collection goes through that type's `CollectionSource` (`loco-apps/src/source.rs`). The lake implements the same trait, so `/data` and `/data/query` call one interface. No registration is still 501 `no source for collection {address}` after the access check and before the required-value check. An address both sides declare is 409 after that same access check. A registered source declares verbs and query filters; anything else is `unsupported` (400 on a verb, that query inside the batch) and is never a widened result. The check then requires this integration's secrets and variables (400 `required configuration is not set`). Live records are `{id, fields}`: `id` is the upstream id, and other system fields are omitted. Filtering or ordering by an undeclared system field is `unsupported`. An upstream failure is 502 `upstream {status}: {message}` on a verb and kind `upstream` on a query, and the message does not echo the request. The production `SourceRegistry` starts empty. The Hurl runner registers the fixture source. The read and write call sites keep the seam for a cache and for the #117 sidecar merge; neither is built. `delete_dataset` stays on the lake adapter. Connection secret and variable values are keyed per integration (below).

`secret` and `variable` declare named configuration a version needs (`label`, `description`, `required`). A variable may also set `default` (a string; empty means none). A secret has no `default` and no `value` — a body carrying either is a 400, because the declaration never holds the value. A secret and a variable may not share a name in one version.

Secret and variable values are lake records on the dataset, in the reserved collections `$secrets` and `$variables`. The record id of a loose declaration is the canonical declaration reference, stored as text with no encoding: bare for this project (`consumer_key`), `{account}/{project}.{name}` for a dependency (`alice/bricklink.consumer_key`). A qualified name for this project (`alice/shop.label_prefix`) is the same row as the bare name. A connection value's id is `{qualified integration}:{declaration name}`: `sf_east:token` for this project's integration, `alice/pkg.store:consumer_key` for a dependency's. A name that contains `:` is a connection value and never a loose declaration. A qualified name for this project's own integration (`alice/shop.sf_east:token`) stores as `sf_east:token`. `DELETE` does not look the declaration up again; it strips a `{project}.` prefix and otherwise uses the path string, so the colon stays and `alice/shop.alice/pkg.store:consumer_key` deletes `alice/pkg.store:consumer_key`. `/` and `.` stay in the id. A collection name is one or more of `a-z`, `0-9`, `_`, `.`, and `-`. `create_collection` returns 400 for anything else, so `$` cannot be declared. A name that contains `$` does not resolve either: `GET /data/$secrets/list` is 404 (`unknown collection`) and `POST /data/query` reports `unknown_collection`. A real collection's lake key is `{owner}.{name}`, which is never the literal `$secrets`.

`SecretStore` (`loco-apps/src/values/`) is plaintext in and plaintext out, so a later cloud store can replace the lake without a second shape. `AppState` holds `Arc<dyn SecretStore>` beside the shared `Arc<dyn DataAdapter>`. The one impl is `LakeSecretStore`: AES-256-GCM, a random 12-byte nonce per write, additional data bound to the dataset id and the canonical name, fields `nonce` and `ciphertext` as standard base64. `LOCO_SECRET_KEY` is standard base64 of exactly 32 bytes (`openssl rand -base64 32`); surrounding whitespace is ignored. Unset or malformed, the process still boots and `PUT /config/secret` returns 503 naming `LOCO_SECRET_KEY`. `DELETE` of a stored secret does not need the key. `get` has no route. An ordinary action calls it for its package's loose declarations, on the installing dataset. A type action calls it for the connection it was addressed to (see "Actions"). The required-value check before a run uses `list` — a row exists, or it does not — and does not decrypt, so an empty stored secret counts as set and the check cannot reveal the plaintext. Variables are a plain `value` string in `$variables` and do not need the key or a trait. Crypto for values is `src/values/`, not `auth/secret.rs` (that one hashes passwords and API keys).

`PUT` and `DELETE /config/secret/{account}/{project}/{dataset}/{name}` — and the same paths under `/variable` — take `{"value":"…"}`. `name` is that reference, percent-encoded as one segment (`alice%2Fbricklink.consumer_key`). Writes require developer or org owner, and the name must be declared by some version of the project, itself or a direct dependency. A name with `:` is a connection value: that version must see the integration (this project or a direct dependency) and the integration's type must declare the name. A connection variable falls back to the type's default. A `DELETE` whose declaration is already gone still removes the row. `GET .../list` is any project role, editor included. A secret row is `{name, project, set, updated_at}` and never the value, so the list uses the same gate as a variable read. A connection row also has `integration`, the canonical qualified integration from the storing project's view (`sf_east`, or `alice/pkg.store`) — the whole string, not only the bare segment a collection listing uses. `name` stays the declaration name. `project` is the integration's owning project. A loose row omits `integration`. A variable row adds `value` and, when one applies, `source` of `value` or `default` (the declaration's default when nothing is set; a connection variable uses the type's default). The list is the declarations of the versions this dataset's sites pin; a dataset no site pins lists nothing, even if a value was stored for it. Deleting a dataset or a project calls `SecretStore::delete_dataset` and then `DataAdapter::delete_dataset`, inside the same purge. The secret-store call is what a later cloud impl cleans up; the lake impl deletes `$secrets` rows the purge would remove anyway, and the lake purge still removes `$variables` and every other collection. If the secret store fails, the lake is not purged. A failed purge is reported left behind and keeps the dataset and project records.

An inline `object`'s `name:` is snake_case (`collection_grant`, `action_param`, `action_param_option`, `integration_secret`, `integration_variable`, `integration_collection`, `integration_field`, `integration_field_option`, `integration_action`, `integration_action_param`, `integration_action_param_option`); codegen PascalCases it into the generated struct name. An action param's `options` object is `action_param_option` — the same `{ value, label }` shape as a field option — so the generated struct is `ActionParamOption` and does not collide with `FieldOption`. The param object itself is `action_param` (`ActionParam`), not a schema type. A type action's params and an integration field's options use their own object names for the same reason. An object name shared by two types (`integration_collection` on both the type and the integration) is emitted once when the properties match. Two shapes for one name are a compile error. A list of objects may itself hold a list of objects; codegen rejects only a list directly inside a list.

### Versions, sites, datasets

- A **version** is a schema snapshot under `{project}/versions/{version}/`. A version whose name ends in `-dev` is a draft (`0.0.1-dev`); only drafts accept `/schema` writes. Any other name — `1.0.0`, `my-app`, `0-draft` — is published. A version exists when it has a manifest, which only `/config` writes: every `/schema` route, read or write, on a version without one is a 404 for a member (`unknown version: {project}@{version}`) from `VersionScope` / `VersionReadScope`, before the body is read; a non-member reading through a site gets the 403 of a version that assigns `public` nothing. Each write re-checks under `PINS` (`VersionSchema::write_guard`), so one racing a version delete is either swept by it or refused — never left as an orphan tree.
- **Names `/config` creates** are checked on write (`http/names.rs`): a project, dataset, or site name is `[a-z0-9_]` starting with a letter or `_` (the account-handle charset); a version name is `[a-z0-9._-]`, not starting with `.` — no `@`, which a dependency uses as its separator. Both at most 63 characters (a site name is a host label). A bad name is a 400 naming the rule, on create and on a version copy's target. Boot does not check: names already on disk load whatever they are, and a copy's `from` need only exist.
- A **dataset** is a lake partition. Record keys are `(dataset_id, collection, id)` where `dataset_id` is `{user}/{project}/{dataset_name}`.
- A **site** pins a `version` + `dataset`. Requests identify the site with `X-Project-Id: {user}/{project}` and `X-Site-Id: {site}`. There is no tenant header. Token-less `public` may perform any `/data` verb a permission set the **pinned version's manifest** assigns (`public_permission_sets`) grants. Policy is on the version, not the site: two sites pinning one version cannot disagree. Grants are not on the collection. Unspecified verbs default to false.
- A site's pin must exist: site create/update is a 400 when `version` is not a version of this project or `dataset` is not one of its datasets. Deleting a dataset or version a site still pins is a 409 naming the sites — re-pin or delete them first; project delete still cascades everything. Check and write run under one process-wide lock (`PINS` in `http/project_config.rs`), since each store's writer lock covers only that store.

Creating a project via `/config` bootstraps `0.0.1-dev`, a `dev` dataset, and a `dev` site.

`POST /config/version/{user}/{project}` creates a version. With `{"version": "0.0.1"}` it is empty; with `{"version": "0.0.1", "from": "0.0.1-dev"}` it snapshots the source version's collections, fields, fieldsets, permission sets, secrets, variables, actions, integration types, integrations, and manifest into the new id — the publish primitive. An action's params travel with the action document. A type's secrets, variables, collections, fields, and actions travel inside the type document. An integration's custom collections and fields travel inside the integration document. The target may be published: copying *into* a non-draft version is how it gets its content, which is why the copy lives on `/config` and not behind `VersionSchema`'s draft gate. The version's file trees (its `bundle`) are copied too — metadata is a version directory, not only its YAML. Datasets, sites, records, and secret and variable values are never copied.

### Manifests and dependency visibility

Each version has a `manifest` instance declaring `dependencies` as `{user}/{project}@{version}` strings and `public_permission_sets` as the names of the permission sets this version assigns to `public`. A consuming version opts into a set a dependency ships by naming it here, qualified (`acme/crm.public_contacts`) — a bare name is the version's own set (see "Name resolution"). `manifest` is a regular schema type — loco-gen treats it no differently than `collection` or `site`.

Dependencies are checked on write (`PUT /schema/.../manifest`, and again when `POST /config/version` copies a manifest): each must be `{account}/{project}@{version}`, name an existing version of **another** project, and name no project twice — a qualified name carries no version, so one project at two versions could not both be addressed. A bad entry is a 400 and nothing is stored. A new entry must also name a project the writer can read — any role on it, or owner of its org (`project_access`) — until projects can be marked installable; one that exists but is not readable gets the same 400, body for body, as one that does not exist. An entry the stored manifest already names is not re-checked for access, and a version copy checks only shape and existence: it is a snapshot, declaring nothing the source does not. Reads through a declared dependency do not check access. Boot does not check: a manifest on disk with bad entries loads, and reads skip the malformed ones. A version another project depends on cannot be deleted, nor can its project (409 naming the dependents); these checks run under the same `PINS` lock as site pins.

Dependency grammar and the scoped view live in `loco-apps/src/http/version_schema.rs` (`VersionSchema`). Reads see the version itself plus **direct** dependencies only (not transitive). Writes go to the version's own project, and only when the `VersionSchema` was constructed writable and the version is a draft.

`ProjectConfig` (`http/project_config.rs`) is the same idea for unversioned config: projects, datasets, sites, version create/delete.

#### Name resolution

**Rule: an unqualified name always means _self_ — the project that owns the running
version. A dependency's collection, field, fieldset, permission set, secret,
variable, action, integration type, or integration must be named fully qualified
(`{user}/{project}.{name}`) to be reachable.**

The point is that installing a dependency can never silently change what an existing
bare name resolves to. Resolution is a property of the name, not of manifest order.

It holds everywhere a name is looked up. `VersionSchema` (`http/version_schema.rs`)
resolves every name through `split`: bare → self, qualified → that project, which must
be self or a **direct** dependency; anything else is not found (404 on `/schema` and
`/data`, `unknown_collection` / `unknown_field` in `/data/query`). No lookup walks the
dependency list for a match, so two deps that share a name are both addressable.

`split` / `split_qualified` cut on the first `.`. A project id contains no `.`, so the
rest may: `acme/crm.foo.bar` is project `acme/crm`, name `foo.bar`. A `:` in that
local addresses an integration (`sf_east:account`, `ben/sync.sf_east:account`). Both
sides of the `:` must be collection names. A name with no `:` is an ordinary
collection or action and never reaches a type's lists or an integration's custom
collections. A name with `:` never falls through to an ordinary one. The integration's
type is resolved from the version that declares the integration, which may depend on
a type the caller does not itself depend on. Standard collections and type actions
are entries on the type document. Custom collections are entries on the integration
document.

- **Collections** — `/schema/.../collection/{name}` and `/data/{collection}/…` take the
  qualified name percent-encoded as one segment: `/data/acme%2Fcrm.contacts/list`
  (`loco-client`'s `data('acme/crm.contacts')` encodes it). A qualified name for self
  equals the bare one. Records of a dependency's collection live in the site's dataset
  under the lake key `{owner_project}.{name}`. A name containing `$` does not resolve
  (`$secrets`, `$variables`). An integration address is the same one segment
  (`/data/sf_east:account/list`, `/data/ben%2Fsync.sf_east:account/list`; `%3A` decodes
  to the same `:`). It resolves to a standard collection on the integration's type
  document, or a custom collection on the integration document. One address names one
  document. A write that would leave both — a custom collection the type already
  offers, a type change onto a custom name, a standard collection an integration of
  that type in the same version already declares, or a manifest (including a version
  copy) whose dependency type offers a custom name — is 400 naming both documents,
  and stores nothing. Omitting the collections list, or re-sending the stored type
  without naming collections, does not re-check. A collision already on disk resolves
  with `project` and `local` and picks neither document, so there is no lake key.
  `/data` answers 409 `ambiguous address {name}: the type offers it and the integration declares it`
  after the same access check as a source call, including `GET .../fields`. A caller
  without a grant is 403. `/data/query` reports that sentence as
  `ambiguous_address` after `forbidden`, beside `no_source`. The listing omits
  both rows. An action address is resolved or missing and is not made
  ambiguous by a collection of the same string. An unambiguous integration
  collection is not a lake row. `/data` and `/data/query` route it to the
  type's `CollectionSource` (`src/source.rs`), keyed by `(type project, type name)`.
  No registration is 501 `no source for collection {address}` after auth and
  before the required-value check, and does not read the lake. A registered
  source is checked for the verb or the query shape first (`unsupported`),
  then for this integration's required secrets and variables, then called
  with that integration's `Connection`. Live JSON is `{id, fields}` only.
  `{address}` is bare for the running project and `{project}.{local}` for a
  dependency. `GET /data/{collection}/fields` is metadata and returns the inline
  fields (name, type, label, description, required, options). It does not call
  the source.
  `/schema/.../collection/{address}` stays the ordinary document lookup, so
  `sf_east:account` is not found there.
- **Fields and fieldsets** belong to the collection's owner. `acme/crm.contacts` has the
  fields acme/crm declares on `contacts`; neither the running project nor another
  dependency adds to it by declaring fields under the same collection name. A fieldset
  on a dependency's collection is named qualified too:
  `/schema/.../fieldset/acme%2Fcrm.contacts/acme%2Fcrm.summary`. An integration
  collection's fields are reachable because the address resolved. The caller
  still writes the owner's name (`alice/pkg.status` on a standard collection,
  bare `body` when the caller owns the custom collection). The owner does not
  also have to be a direct dependency: `field_in` keeps that check for ordinary
  collections only. A name without `:` does not bring the type package's other
  collections into scope.
- **Actions** follow the same owner rule. `/actions/{name}` and
  `/schema/.../action/{name}` take a bare name for this version's own action and a
  dependency's qualified and percent-encoded (`loco%2Fbricklink.sync_orders`). Params
  are the list on that action, so a consumer sees the owner's params and has no
  separate write that could add one. The handler registry is keyed by that owning
  project and the bare name, so a same-named declaration in another project does
  not run this project's handler. A `:` addresses a type action the same way
  (`sf_east:set_owner`), read from that integration's type document. Input is
  validated first. The handler is keyed by `(type project, type name, action name)`.
  A missing handler is 501 `no handler for action {type_project}.{type_name}.{name}`
  before the required-value check, so an unimplemented type action does not demand
  configuration. When a handler is registered, the check is this integration's
  required secrets and variables only — never the loose version-level declarations —
  and the handler runs with that connection. A collection of the same string
  does not make the action ambiguous. `/schema/.../action/{address}` stays the ordinary
  action document and does not resolve the address. `GET /actions` does.
- **Integration types and integrations** follow the same owner rule.
  `/schema/.../integration_type/{name}` and `/schema/.../integration/{name}` take a
  bare name for this version and a dependency's qualified and percent-encoded
  (`alice%2Fpkg.bricklink`, `alice%2Fpkg.store`). Standard collections, fields, and
  type actions are lists on the type document. Custom collections and fields are
  a list on the integration document. A write uses the path name as this
  version's document, so a qualified name that is not one of its documents is
  not found. An address `{integration}:{name}` is resolved on `/data`,
  `/actions`, `/data/query`, and grants, through the integration named before the
  colon. The lists themselves stay on the type or the integration.
- **Handler config** — `ActionContext::secret` and `variable` do not use `split`.
  An ordinary action receives `ActionContext` and reads the owning project's loose
  declarations (the registry key) and the values on the request's dataset. A bare
  name inside that handler is that package's declaration, including when the
  installing project declares the same name. A name that package does not declare
  is 500 `not declared by this package`. A type action receives `TypeActionContext`.
  `connection` is required: that integration's values plus the shared HTTP client.
  `secret` and `variable` on the context forward to the connection and do not read
  a loose declaration. A name the integration type does not declare is 500 naming
  that type and its reference (`not declared by integration type 'alice/pkg.warehouse'`).
  `Connection::new` takes the dataset, the declarations, the secret store, the lake
  adapter, and the HTTP client, and a collection source builds one the same way,
  without the action runner. The type-action context still carries the site's `VersionSchema`
  and the raw `DataAdapter` (a handler can patch records). It does not carry the
  secret store.
- **Permission sets** in `public_permission_sets`: bare is this version's own set; a
  dependency's is opted into as `acme/crm.public_contacts`. A consumer's set may share a
  name with a dependency's — they are different sets.
- **Grants** inside a permission set resolve from the set's own project
  (`collection_grant_matches`, `http/authz.rs`): a bare grant `contacts` in the
  consumer's set opens only the consumer's `contacts`, and in a set a dependency ships
  only the dependency's. A qualified grant names its owner exactly. The split is the
  first `.`. The name compared is the address local (`sf_east:account` or `orders`)
  and the project is the address root — who declared the integration, or who owns
  the ordinary collection — not the type that owns the fields. A bare `account`
  grant does not open `sf_east:account`. `sf_east:account` does not open
  `sf_west:account`.
- **Listings** (`permission_set/list`, `secret/list`, `variable/list`, `action/list`,
  `integration_type/list`, `integration/list`) span self + direct deps and return each
  item's `project`, from which a client builds the qualified name. A bare secret,
  variable, or action name is this version's own; a dependency's is
  `loco/bricklink.consumer_key`. An action's params are on the action, in the order
  the action declares them. Schema `action/list` stays those ordinary action documents.
  `collection/list` is one row per address: ordinary collections first, then each
  integration's standard collections and its custom collections, in the document's
  list order. A name both the type and the integration declare is omitted. Each row
  has `project` (the address root), `name`, `owner` (who owns the fields),
  `label_plural`, and `integration` (omitted on an ordinary collection). `GET /actions`
  is the same shape for actions, including `params`.

### Fieldsets

A fieldset is an ordered named subset of a collection's fields. `auto_add: true` marks the set that new fields are appended to. `VersionSchema::fields(collection)` returns the owner's fields in the owner's auto-add fieldset order (then leftover fields alphabetically). Studio uses that order for the collection table; record create/edit forms currently render the same list as returned by `/schema/.../field/{collection}/list`. The edit form sends only the fields the user changed, so a stored value the schema no longer accepts does not block saving another field.

## Naming Conventions

These conventions apply to property names in type definitions and variable names in `pathTemplate`s.

- **`id`** — opaque identifier (uuid/number) with no semantic meaning. Immutable once set. Reserved on schema types. Lake records get a UUID `id` stamped by `Record::new_for_insert`. Secret and variable value rows are the exception: `upsert` stores the declaration reference as the id.
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
| `/data` | Record CRUD on `/data/{collection}/…` — one percent-encoded segment: bare for the site's own collection, a dependency's qualified (`acme%2Fcrm.contacts`), or `{integration}:{name}` (`sf_east:account`, `ben%2Fsync.sf_east:account`). An address that names both a standard collection and a custom collection is 409 `ambiguous address …` after the access check; a caller without a grant is 403. `GET /data/{collection}/fields` returns an unambiguous address's fields (ordinary, the type's, or the custom collection's) and is 409 for an ambiguous address after that read check. List, get, add, update, and delete on an unambiguous integration address go through that type's `CollectionSource` after auth. No registration is 501 `no source for collection {address}` before the required-value check, and does not read the lake. A registered source that does not declare the verb is 400 `unsupported` before body validation. Then the body is validated, then this integration's required values (400 `required configuration is not set`), then the call. An upstream failure is 502 and does not echo the request. A live record is `{id, fields}`. `POST /data/query` is named batched reads ([`docs/query.md`](docs/query.md)); an ambiguous address is `ambiguous_address` inside the 200 after `forbidden`, and an unambiguous integration collection with no source is `no_source`. A registered source that cannot honor the query is `unsupported` on that query. An upstream failure on one integration query is kind `upstream` and the rest of the batch still returns. Lake queries in the batch share one snapshot. Site-scoped via headers. Strict validation on write; diagnostics on read. |
| `/actions` | Site-scoped, like `/data`. `GET /actions` and `GET /actions/{name}` return one row per address (ordinary actions and `{integration}:{name}` type actions) with `project`, `name`, `owner`, `params`, and `integration` when the address names a connection. An action address is resolved or missing. `POST /actions/{name}` body `{"input":{…}}` validates that input. An ordinary action runs the handler registered for its owning project and bare name. A type action validates input, then runs the handler registered for `(type project, type name, action name)` with that integration's connection. A missing handler is 501 `no handler for action {type_project}.{type_name}.{name}` before the required-value check. The check is this integration's required secrets and variables only. See "Actions". |
| `/schema` | Versioned metadata CRUD (manifest, collections, fields, fieldsets, permission sets, secrets, variables, actions, integration types, integrations, bundle). An action's params are part of the action document. A type's secrets, variables, and standard collections, fields, and actions are part of the type. An integration's custom collections and fields are part of the integration. See [`docs/integrations.md`](docs/integrations.md). |
| `/config` | Unversioned project / dataset / site / version lifecycle, plus per-dataset secret and variable values (`/config/secret`, `/config/variable`), including an integration-qualified name (`sf_east:token`, `alice%2Fpkg.store:consumer_key`). |
| `/auth` | Login, logout, `/me` (self), signup (`POST /users`), update/delete (self), API keys. |
| *(fallback)* | Files from the request's site's **pinned version** bundle. `handlers/hosting.rs`. |

CORS is `*` origin, method, and header. No cookies; clients send `Authorization: Bearer`. Studio's Vite proxy is unchanged.

### Site hosting

`http/host.rs` resolves the request's site from `Host` before routing; `handlers/hosting.rs` is the router fallback that serves that site's pinned version bundle. Full model: [`docs/hosting.md`](docs/hosting.md).

- **Which site.** `{site}.{project}.{account}.<listen-host>` — `www.blog.ben.localhost:3000` is site `www` on `ben/blog`. The listen host is not configured: a host is a site host exactly when its first three labels name a site that exists. Anything else (the apex, an IP, a stale subdomain) is no site, and then `LOCO_DEFAULT_SITE={account}/{project}/{site}` decides whether `/` serves anything. There is no default for it in the binary and there must not be one — a Loco process is not a Studio process.
- **Site headers.** Both cases fill in absent `X-Project-Id` / `X-Site-Id`, so a hosted frontend need not know its own address. A subdomain additionally *pins* them: a header naming another site is a 400. The apex default does not — a sent header wins, which is what keeps the apex usable by Studio and local Vite apps.
- **Serving.** Reserved prefixes (`/data` `/schema` `/config` `/auth` `/actions`) always answer JSON, never HTML, so a mistyped API path stays a JSON 404. Otherwise `/` is the tree's `index.html`; a miss falls back to it when the path is extensionless or `Accept` asks for HTML, and 404s otherwise (a missing hashed asset must not come back as an HTML shell). Files are public — no token. A published version's assets go out `immutable`; `index.html` and everything in a draft revalidate, because the pin is what moves.
- **Missing bundle** is an ordinary state: one warning at boot if the default site has none, then `/` 404s. Never a boot failure.

Studio is not special-cased anywhere: it is a bundle on a `loco/studio` version like any other frontend, and no path in `server.rs` names it. Vite-dev on `:5174` stays the inner loop.

Handlers sit on request extractors in `http/scope/`:

- `SiteScope` — resolves project + site from headers, attaches auth (or `public`), builds a **read-only** `VersionSchema` for the site's pinned version. Home of `require_authenticated`, `require_developer`, `require_can_write_data`. `/data` and `/actions` both use it. Access is membership, not the site.
- `VersionScope` — authenticated identity plus a **writable** `VersionSchema` for the path triple. Requires developer (or org owner) on the path project. Used by `/schema` writes.
- `VersionReadScope` — read-only `VersionSchema` for GET `/schema`. Developer/editor on the path project (any version, no site headers). `public` (and authenticated non-members) on a site whose pinned version assigns at least one permission set to `public` (pinned version only; `X-Project-Id` + `X-Site-Id` required).
- `ConfigProjectScope` / `ConfigMemberScope` / `ConfigUserScope` — `/config` routes. Project-targeted writes require developer. Secret and variable value reads (`ConfigMemberScope`) allow any project role. Project list/create/org do not need site headers.
- `CollectionScope` / `RecordScope` — `/data` routes. Authenticated writes need editor or developer. Token-less `public` may list/get/insert/update/delete when a permission set the pinned version's manifest assigns to `public` grants that verb on that address. The grant matches `(local, address root)`. `GET /data/{collection}/fields` follows the read rule and, for an unambiguous integration address, returns the standard or custom fields. It is how a hosted frontend reads field metadata (labels, `options`) without knowing its version: same list, order, and shape as the schema field list for that address, but the version comes from the site pin, so re-pinning the site changes the answer. An ambiguous address is 409 after that check. Data verbs on an unambiguous integration address with no registered source return 501 after that check and do not validate or touch the lake. A registered source is capability-checked before the call. Writes validate the body, then this integration's required values, then call. Reads check the required values, call, then validate the returned records. `GET .../fields` stays metadata and does not call the source.
- `POST /data/query` takes a bare `SiteScope` and authorizes each query on its own with `SiteScope::may_read_collection` (the same read rule, on `(local, address root)`); a denied or invalid query is an error result inside a 200, not a failed request. An ambiguous address is `ambiguous_address` after that grant check, beside `no_source` when no source is registered. An integration query the caller may read is planned against the address's fields (the owner need not be a direct dependency), then checked against the source's capabilities, then this integration's required values, then run on that source. The cursor hash binds to the canonical address. A source that supports query declares `$id`, because the server appends it as a tie-breaker. Lake queries still share one adapter snapshot through `LakeSource`. Parsing, strict name resolution, and cursors are in `src/query.rs`.

Membership: `org_members (org, identity, owner|member)` and `project_members (project, identity, developer|editor)`. Effective project access = org owner ∪ project role, plus implicit developer when the identity owns the person account (`alice` → `alice/*`).

Login (`POST /auth/login`) is global — it does not use `X-Site-Id` to find the user. Body is `{ "username", "password" }`. Seeded identities (`alice`, `bob`) have password `password`. Org accounts (`loco`) cannot log in. Sessions and API keys hang off the identity and both work as `Authorization: Bearer …`. Only a request with **no** `Authorization` header is `public`; one whose header is malformed, or names an unknown, expired, or logged-out session or a revoked key, is a 401 on every route (`auth::session_or_public`), never a quiet fall back to the public view. `GET /auth/me` is authenticated self-read. `POST /auth/users` is self-service signup (no token; password required). `PUT` / `DELETE /auth/users/{id}` are self only — org owners manage membership, not the person account. There is no `/auth/users/list`. Login auto-creates an unknown handle only when `LOCO_AUTH_AUTO_CREATE=1` (Hurl) or `cfg(test)`. Credentials are hashed at rest by `auth/secret.rs`: passwords with argon2id, API keys with SHA-256 (the plaintext key is returned once, from `POST /auth/api-keys`). Local `auth/*.json` files written before hashing are re-hashed in place on load. Sessions expire `SESSION_TTL_DAYS` (7) after login — there is no refresh, so the client logs in again. `validate_session` returns `SessionExpired` past that point and drops the session from the cache and from disk; expired sessions left on disk are swept at startup. API keys do not expire; they are revoked.

### Validation

Lives in `loco-apps/src/validation.rs`, not in the lake. Checks unknown fields, scalar type mismatches (`string` / `integer` / `float` / `boolean`), and a `string` value outside the field's `options` when it declares any (`invalid_option`; exact match on `value`, so `""` is rejected too). `Null` is allowed for any type that is not `required`. A field's `type` must be one of those four scalars (`FIELD_TYPES`): `/schema` field create and update reject anything else with a 400, and so does each param on an action create or on a PUT that replaces `params` — the same check, so the message still says `unknown field type`. Boot does not check, so a field YAML written earlier with another type (`list`) still loads, and its values pass. An action loaded from disk is likewise not re-checked, and a version copy does not re-check its params. A field with `required: true` must have a value on create: missing, `null`, or (for a `string`) `""` is `required`. `""` counts as blank because it is what a cleared text input sends. An update checks a required field only when the patch names it. Reads — `/data` get/list and `/data/query` — report a gap as a warning, never an error, so making a field required later does not break records written before; a query with `fields` checks only the fields it read. An integration collection uses the inline fields on the type or the integration (`validate_inline_records`), the same diagnostic kinds, and names the collection by its canonical address. Ordinary collections still use `Field` documents. The handler projects a live record before that check. Lake projection stays inside the adapter.

Action input uses that same walk (`validate_action_input`, and `validate_type_action_input` for a type action). The diagnostic `kind` is unchanged (`unknown_field`, `type_mismatch`, `invalid_option`, `required`). The message says `param` and names the action (`action 'sf_east:set_owner'` for an integration address), where a record says `field` and names the collection. A JSON array or object is a `type_mismatch`: values are scalars.

### Actions

An action is a declared operation on a version (`label`, `description`, `params`). Each param is the field vocabulary (`name`, `type`, `label`, `description`, `required`, `options`) and lives in the action document, in declared order. `/schema` writes the action only. Output is not declared: the handler returns JSON, and the description says what that JSON is. There is no workflow language, trigger, or schedule.

`GET /actions` and `GET /actions/{name}` are how a hosted UI builds the form, the way `GET /data/{collection}/fields` reads field metadata. The read rule is that one, lifted off a single collection: data access, or a read grant on any collection the pinned version shows. A version with nothing readable is members only. Params come back in the order the action declares them, and they are the owning project's params.

`POST /actions/{name}` runs it. The body is `{"input":{…}}`; a missing `input` is `{}`. The runner validates input first. Any error is the `/data` create 400 (`validation failed` plus diagnostics) and the handler is not called. Once a handler is registered, the run is 400 `required configuration is not set` with one diagnostic per missing name (`secret 'token' is not set`, `variable 'region' is not set`) and the handler is not called. For an ordinary action that is every required secret and variable the owning project's visible version declares. For a type action it is this integration's declarations only, and a loose declaration of the same name does not satisfy it. A variable whose declaration has a non-empty default counts as set. The check uses `list` (a row exists, or not) and does not decrypt, so an empty stored secret is set and the 400 does not reveal a secret's plaintext. A missing handler is 501 before this check.

An ordinary action's handler receives an owned `ActionContext`: the site's dataset id, the `DataAdapter`, a read-only `VersionSchema` for the pinned version, the caller's identity, the validated input, plus `secret(name)`, `variable(name)`, and `http()`. `secret` and `variable` take the package's bare declaration name. The declaration comes from the owning project (the registry key), not through `split`, so a secret or variable the installer declares under the same bare name is a different row and is not returned. The value is always the request dataset's. A stored variable wins, including `""`; with no row, a non-empty default is returned. The package's own datasets are not opened. Asking for a name the package does not declare is a bug in the handler and is 500 `not declared by this package`.

A type action's handler receives `TypeActionContext`: the same dataset id, `DataAdapter`, schema, caller, and input, plus a required `connection`. There is no secret store on that context. `secret` and `variable` forward to the connection and read `{qualified integration}:{name}` on the request dataset. A loose secret of the same name, and another integration's row, are not returned. A name the integration type does not declare is 500 naming that type (`not declared by integration type '{reference}'`). The connection is the dataset id, the canonical integration, those two reads, and the shared HTTP client. `Connection::new` takes those stores and the client, and a collection source builds one the same way. The context's `DataAdapter` is the site's lake adapter, the same reach an ordinary handler has.

Handlers are async and run on the request task. `http()` is a process-wide `reqwest` client: 5s to connect, 30s for the whole call, and no proxy (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, or the OS settings). A redirect is followed only when the next URL keeps the same scheme, host, and port, at most ten times; the eleventh fails the call. `DataAdapter` calls stay synchronous on that task and finish without awaiting, so no lock is held across an `.await`. An upstream failure is 502 `upstream {status}: {message}`, where `status` is `0` when the call got no response. The message is returned to the caller verbatim, so a handler must not put secret material in it, or an upstream body that echoes the request. `From<reqwest::Error>` drops the URL reqwest would append. It is never 500. Reading a secret when `LOCO_SECRET_KEY` is missing or malformed is 503, the same answer as `PUT /config/secret`.

Who may run: an authenticated editor or developer, via `require_can_write_data`. That is the refusal a `/data` add gives a caller who cannot write. Token-less `public` is never allowed, including when a public permission set grants `create` on a collection. An unknown name is 404 (`action not found: {name}`) before that check. A declared ordinary action with no handler is 501 `no handler for action {project}.{name}`. A resolved type action with no handler is 501 `no handler for action {type_project}.{type_name}.{name}` after input validation and before the required-value check. A registered type action runs with a `TypeActionContext` whose connection is that integration. An action address is never ambiguous.

Ordinary handlers are registered in code against `(owning project, bare name)`. Type-action handlers are registered against `(owning project, type name, action name)`. The production binary registers none: `build_app` uses empty registries. A source registry (`SourceRegistry`, keyed by owning project and type name) holds `Arc<dyn CollectionSource>`. The production binary starts it empty, so an integration collection with no registration is 501 `no source for collection {address}` before the required-value check. The Hurl runner registers `WarehouseSource` for `alice/pkg`'s `warehouse` type in the collection-source suite only. The lake is `LakeSource` on `AppState` and is always present. The type-action registry is dispatched. `POST /actions/{integration}:{name}` validates input, then either runs the registered handler with the connection or returns 501 when none is registered. It does not run the loose required-secret check. The Hurl fixture handlers (`alice/fixture` / `echo`, `alice/pkg` / `pull`, and `alice/pkg` / `warehouse` / `read`) are registered by the test runner, so they are not in the server binary. `pull`'s upstream URL is a variable the installing dataset sets; the declaration's default points nowhere. `read` returns the connection it was addressed to.

The lake has no transactions. A handler that fails after it has written leaves those writes in place. That partial failure is the contract: handlers must be safe to re-run, and a handler error should say what was already written. The handler maps that to 400 (bad input it found itself), 409 (conflict), 502 (upstream), 503 (`LOCO_SECRET_KEY`), or 500, with diagnostics when it has them — the same body shape as `/data`. Handlers update records by patch and never replace a record's `fields` wholesale, because an installer may have added fields the owning package's code does not know about (#117).

## Key Patterns

- **Rust keyword escaping**: Codegen emits `r#type` (etc.) for property names that are Rust keywords. See `rust_ident()` in `codegen.rs`.
- **Error types**: Each crate has its own error enum — `loco_gen_schema::Error`, `loco_schema_runtime::Error`, `loco_lake::Error`.
- **Tests**: Unit tests are co-located (`#[cfg(test)] mod tests`). Filesystem tests use `tempfile`. API tests are Hurl suites under `loco-apps/tests/suites/`, driven by `tests/hurl_runner.rs`.
- **Thread safety**: `InstanceStore` uses `RwLock<BTreeMap<...>>` for reads, and every mutation holds a per-store writer mutex across check, persist, and cache update (`FileTreeStore` too). Read-modify-write goes through `InstanceStore::update_with`, never `get` then `update`. No store locks another, so nothing nests: a caller touching several stores takes them one after another. Secret and variable writes take `PINS`, then the lake adapter lock: `DataAdapter::upsert` holds that lock across the existence check and the write. `InMemoryAdapter` uses `RwLock<HashMap<...>>`. `SqliteAdapter` uses `Mutex<Connection>`.

## Frontend Apps

All frontend apps use the same stack:

- **Vite** with `@vitejs/plugin-react`
- **React** (functional components, hooks)
- **React Router** (`react-router-dom`, `createHashRouter`)
- **TanStack Query**
- API client: `loco-client` (below) for new apps. Studio still has its own `src/api.js` (plain JS, not a hook) and `src/auth.js`; moving it onto `loco-client` is a follow-up.
- Components in `src/components/` as `.jsx` files
- Dev-only Vite proxy of `/auth` `/config` `/schema` `/data` `/actions` to `localhost:3000` (no `/api` prefix). Studio and `examples/brickos-inventory` both proxy `/actions`. `API_ORIGIN` in `loco-studio/src/config.js` is `''` — same origin — which is what both the proxy and a hosted bundle need; set it absolute only for a deliberately cross-origin build like `examples/public-page`

### Frontend locations

- `loco-studio/` — Schema + record UI (port 5174). The token is the person. Schema/config calls do not need site headers. Data calls send `X-Project-Id` / `X-Site-Id` for the browsed site.
- `loco-ui/` — Reusable field library (no library build; consumed via npm workspaces). Playground at port 5175 (`npm run dev -w loco-ui`).
- `loco-client/` — API client, consumed the same way. Used by `examples/brickos-inventory`.
- `examples/brickos-inventory/` — Hosted BrickOS batch editor (port 5177 in dev). A workspace, so it resolves `loco-client` from the repo.

### loco-client

`createClient({ origin = '', projectId, siteId })`: plain JS with JSDoc, no framework dependency, no build step (`loco-client/src/index.js`). The app keeps its data layer (TanStack Query) and calls the client from it.

- **Session.** `login`, `logout`, `me`, `isLoggedIn`. The token is stored per API origin (`loco_session:{origin|same-origin}`) and sent as `Authorization: Bearer`. A 401 on a request that carried the token drops it. `onSessionChange(listener)` hears login, logout, and that drop, and returns an unsubscribe, so `useSyncExternalStore(client.onSessionChange, client.isLoggedIn)` is the whole of an app's session state.
- **Site headers** only when `projectId` / `siteId` are given — a dev server on another host. A hosted bundle omits them and the server infers the site from `Host`.
- **Records.** `data(collection)` → `{ list, get, add, update, remove, fields }`. `fields()` is `GET /data/{collection}/fields`, the pinned version's field metadata with `options`.
- **Queries.** `query(batch)` is one `POST /data/query` and resolves to the per-query results, failed ones included. `queryAll(query)` follows one query's cursors to the end and rejects if it fails.
- **Errors** are `LocoError` with `status` (0 when unreachable) and `diagnostics`. The message is the error diagnostics' messages, so a rejected write reads as what was wrong, not `validation failed`.

### loco-ui

Field components for rendering schema metadata. Two layers:

- **Primitives** — `TextField`, `NumberField`, `CheckboxField`, `ToggleField`, `SelectField`. Uniform shell props: `id`, `label`, `description`, `error`, `required`, `disabled`, `value`, `onChange`, plus type-specific props.
- **Dispatcher** — `<Field field={meta} variant?="..." />` picks a primitive from a hardcoded `type → variant → component` registry. `variant` can come from field metadata or be overridden at the call site. A `string` field with non-empty `options` renders `SelectField` whatever its variant, with a blank `—` entry that Studio sends as `null`. A stored value that is not among the options is shown as an extra entry marked `(not an option)`, so the select never displays a different choice than the record holds.

Styling is plain CSS via `.module.css` files co-located with each component. Shared design tokens (`--loco-*` CSS variables) live in `src/_shell/tokens.css` and must be imported once by the consumer (`import 'loco-ui/tokens.css'`).

Collection field metadata (`field.yaml`) has `type` (`string`, `integer`, `float`, or `boolean`), `label`, `required`, and `options` (a list of `{ value, label }`; empty means any string). The dispatcher marks a required field and sets the native `required` on its input — except a `boolean`, where unchecked is `false`, a value. It also reads `description` and `variant` when present.

## Rust Edition

2021 — all crates.
