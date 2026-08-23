# Future ideas

Parking lot for ideas that are still relevant but **not the current plan**. See `ROADMAP.md` for what's built and what's next.

These came from an older 12-phase roadmap. Items that already shipped (runtime YAML loading, write-time validation, `AuthAdapter`, local auth, API keys, studio) are omitted. Names have been updated to match the current model: **sites + datasets**, not a tenant registry.

---

## Schema sources beyond the local filesystem

Schemas are YAML in a known folder structure. A `SchemaSource` trait with pluggable adapters would decide *where* those files come from. The local filesystem adapter is what exists today; this is the rest of that idea.

### Schema source URIs

```
file://schemas/instances/ben/crm           → local filesystem (today)
git://github.com/ben/loco-crm@v0.0.1      → git repo + tag
git://github.com/ben/loco-crm#main        → git repo + branch (drafts)
npm://@ben/loco-crm@0.0.1                 → npm registry
```

- **Schemas are files, versioned by their source** — git tags for published versions, branches for drafts, local filesystem for development. Loco doesn't reinvent version control.
- **Git adapter** — clone/fetch a repo, checkout a ref, load schemas from the working tree
- **npm adapter** — download a package, extract, load schemas
- **Site pinning** — each site already pins a version + dataset. Longer term, a site could pin a remote ref (`ben/crm@v0.0.1` from git) instead of a local draft folder.

### Additional field types

Date, datetime, reference (foreign key), picklist, multi-select, rich text, file/attachment. (A `required` flag and `list` for records belong on the current roadmap, not here.)

---

## API keys (beyond the prototype)

The adapter and `/auth/api-keys` routes exist. The key is shown once at
creation and only its SHA-256 digest is stored. Still open:

- Multiple labeled keys per user ("CLI", "CI pipeline", "MCP") with individual revocation — mostly there
- Last-used timestamps, audit trail for use
- Rate limiting per key
- Derive site/dataset from the key so authenticated clients don't have to send `X-Project-Id` / `X-Site-Id` by hand

---

## MCP server

Expose loco so AI agents can discover and interact with it natively.

- Tools: `create_collection` / `create_field` — define schemas conversationally
- Tools: `insert_record` / `update_record` / `delete_record` — full CRUD
- Tool: `query_records` — list, filter, sort, paginate
- Tool: `describe_schema` — inspect collections and fields
- Tool: `generate_report` — summarize, aggregate, analyze
- Resources: collections and schemas as browsable MCP resources
- Clients: Claude Desktop, Cursor, Claude Code, and others

Depends on auth being real enough that a key can be handed to an MCP client.

---

## Loco CLI

Command-line tool for humans and agents, talking to the same API.

- `loco collections list` / `loco fields list` — inspect schemas
- `loco records list <collection>` — ASCII tables
- `loco records add <collection>` — interactive or flag-based create
- `loco records export <collection> --format csv|json|yaml`
- `loco records import <collection> <file>`
- `loco sites list` / `loco sites create` — site (and dataset) management
- Output formats (table, JSON, CSV, YAML) via `--output`
- Connection profiles (server URL + site for quick switching)

---

## Security model

Target identity model (person/org accounts, project membership, no application-user table) is in [`docs/identity.md`](docs/identity.md). The following is still later.

### Authentication adapters

`AuthAdapter` and the local filesystem adapter exist. Future adapters:

- **Local, with passwords** — email/password on `loco/core.user` with bcrypt. The trait already takes a password; login currently passes `None`.
- **Clerk** — MFA, social login, hosted UI
- **Others** — Auth0, Supabase Auth, Keycloak, generic OIDC/OAuth2
- **Configurable per site** — different sites could use different providers

### Authorization

Today: hardcoded metadata-editor site allowlist + "session user must match path `{user}`".

- Role & permission system — define roles, assign to users, enforce everywhere
- Collection-level security — per-site permissions (read, write, delete) on collections
- Record-level security — ownership, sharing rules, team/role visibility
- Field-level security — hide or make read-only specific fields per role
- Row-level filtering — queries automatically scoped by the caller's permissions
- Audit trail — who changed what, when, on every record

---

## Hosted platform

Loco as a managed service.

- Self-service signup and onboarding
- Per-site (or per-account) resource isolation and usage limits
- Admin dashboard for management (or CLI/MCP-based admin)
- Billing and usage metering
- Custom domains per site
- Managed database backends (Postgres, Turso, etc.)

---

## Namespace marketplace

Namespaces are just folders. If they can be published to git or npm (see schema sources above), the marketplace is discovery and curation — not a custom registry.

- `loco install <namespace>` — resolve a URI, add it to the project, pull schema files
- Dependency resolution — a project file declares dependencies (e.g. `ben/crm` depends on `loco/core`), resolved transitively on install. Manifests already declare direct deps; this is install/publish around that.
- Catalog — web directory or CLI-searchable index of published namespaces
- Templates — starter namespaces: CRM, project management, inventory, HR
- Upgrade paths — `loco upgrade ben/crm@0.0.2` updates the pin, shows a schema diff, flags breaking changes

---

## Scripting engine

Sandboxed TypeScript for user-defined logic, on Deno's Rust crates (`deno_core` / `deno_runtime`).

- Record triggers — on create, update, or delete (before and/or after)
- Sandboxed execution — no filesystem/network by default; grant permissions explicitly
- Script API — a `loco` global with the current record, collection, site, and lake operations
- Script storage — in the lake or as files on a collection/namespace
- Error handling & logging — capture errors, console output, metrics per run
- Execution limits — timeout, memory caps, rate limits

### Event & task engine

- Durable task queue (SQLite or Postgres)
- Cron schedules
- Inbound webhooks that trigger scripts
- Internal event bus for record changes
- Chained tasks (one script's output into the next)
- Retry & dead-letter
- Queryable execution history

---

## File lake

Same adapter pattern as the data lake, for unstructured files. The file lake doesn't store files itself; it wraps backends.

- `FileLakeAdapter` — `upload`, `download`, `delete`, `list`, `get_metadata`
- Local filesystem adapter (dev)
- S3-compatible adapter — AWS S3, MinIO, R2, GCS
- Site- or dataset-scoped storage, same isolation as records
- File metadata in the data lake — name, size, content type, storage path as records, so files are queryable and linkable
- File field type on collections
- Presigned URLs
- Size & type limits, configurable per site or collection

---

## Client SDKs & framework integrations

Thin clients so existing tools (React, Vite, TanStack Query) talk to loco with little boilerplate. Not a UI framework.

### `@loco/client` (TypeScript)

- Typed CRUD, site/project scoped
- Pagination (cursor and offset)
- Filtering & sorting
- Batch operations
- Structured errors matching `ApiResponse`
- Auth: attach site headers, API keys, or tokens
- File-lake helpers and presigned URLs

### `@loco/react`

Hooks wrapping `@loco/client` with TanStack Query. Studio already uses React Query + `api.js`; this would be the extracted, typed version.

- `useLoco(collection)` — client scoped to a collection
- `useLocoQuery(collection, filters?)` — list with cache, pagination, refetch
- `useLocoRecord(collection, id)`
- `useLocoMutation(collection)` — insert/update/delete with invalidation
- `useLocoSchema(collection?)`
- SSR-friendly (Next.js, Remix, etc.)

### Realtime

- SSE as the default (HTTP, proxy-friendly)
- WebSocket upgrade for bidirectional cases (presence, collaborative editing)
- `subscribe(collection, filters?)` → insert/update/delete events
- TanStack Query integration so cached lists update live
- Site-scoped channels

### Other frameworks

Vue composables, Svelte stores, vanilla `@loco/client`, React Native (likely `@loco/react` as-is).

---

## Advanced data features

- Relationships — explicit foreign keys, cascading deletes, referential integrity
- Computed fields — derived from expressions or other fields
- Validation rules beyond type/required — regex, ranges, uniqueness, required-if
- Full-text search
- Bulk operations with transactional guarantees
- Change data capture
- Outbound webhooks on data events

---

## Open questions

- **Agent SDK** — a higher-level library over MCP tools, in Python, TypeScript, or Rust
- **Formula language** — for computed fields, validation rules, and filters (Salesforce formulas / Excel-ish)
- **Multi-adapter per site** — SQLite for a dev site, Postgres for production, chosen per site
- **Snapshot & restore** — export a site's (or dataset's) data and schemas for backup, migration, or cloning
- **GraphQL or query DSL** — joins, aggregations, nested queries beyond list/get
