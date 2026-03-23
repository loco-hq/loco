# Roadmap

Loco is a low-code backend platform for structured data — think Salesforce or Airtable, but built from the ground up for AI agents and tools rather than traditional UIs.

## Vision

Traditional low-code platforms invest heavily in drag-and-drop UIs, form builders, and dashboards. Loco bets that AI tools have made most of that unnecessary. What remains essential is the backend: structured data, schema enforcement, multi-tenancy, security, and reliable APIs. Loco provides the data platform; AI agents and CLIs provide the interface.

## Principles

- **Backend-first** — structured data and schema enforcement are the foundation. UIs are optional; AI tools and CLIs are first-class citizens.
- **Schema-driven** — types and fields are defined declaratively. The system enforces structure so agents and users can trust the data.
- **Multi-tenant by default** — every operation is scoped to a tenant. Security is not bolted on later.
- **Composable namespaces** — functionality is packaged into installable namespaces (like `loco/core`, `ben/crm`). Mix and match to build your app.
- **AI-native interfaces** — MCP servers, CLI tools, and agent APIs are the primary way people interact with the platform.
- **Loose on load, strict on save** — the data lake is schemaless, so reads must tolerate missing fields, extra fields, and type mismatches without crashing. The app does the best it can with what it has. Writes are strict — you cannot save data that doesn't conform to the current schema. This ensures schema evolution never breaks existing data.

---

## Phase 1: Foundation (current)

What we have today.

- [x] YAML-based schema definitions (types + instances)
- [x] Build-time code generation (TypeDef → Rust structs)
- [x] REST API server (Axum) with CRUD endpoints
- [x] Multi-tenant data isolation (header + query param)
- [x] Tenant registry (YAML files)
- [x] Pluggable storage adapters (in-memory, SQLite)
- [x] Namespace convention (`{user}/{project}`)
- [x] Core namespace (`loco/core`) with user collection

## Phase 2: Schema Sources & Runtime Loading

Schemas (collections, fields) are just YAML files in a known folder structure. A `SchemaSource` trait with pluggable adapters determines *where* those files come from. This replaces the current build-time-only codegen with runtime schema loading, and lays the groundwork for the namespace marketplace.

### Schema Source URIs

Each namespace is referenced by a URI that tells loco where to load it from:

```
file://schemas/instances/ben/crm           → local filesystem (today)
git://github.com/ben/loco-crm@v0.0.1      → git repo + tag (future)
git://github.com/ben/loco-crm#main        → git repo + branch (future, for drafts)
npm://@ben/loco-crm@0.0.1                 → npm registry (future)
```

### Namespace Folder Structure

Every namespace, regardless of source, follows the same layout:

```
ben/crm/
├── loco.yaml              # namespace metadata (name, version, dependencies)
├── collections/
│   ├── account.yaml
│   ├── contact.yaml
│   └── opportunity.yaml
└── fields/
    ├── account/
    │   ├── company.yaml
    │   └── active.yaml
    └── contact/
        ├── first_name.yaml
        └── last_name.yaml
```

### Data Model

```
user (ben)
└── project (crm)
    ├── namespace source (file://, git://, npm://)
    │   ├── branches/tags = drafts/versions (managed by git, not loco)
    │   └── folder structure defines collections + fields
    └── tenants (acme, globex, dev-sandbox, ...)
        ├── pinned ref (git tag, branch, or just "local")
        ├── installed namespaces (loco/core@0.1.0, ...)
        └── data (in the lake, isolated per tenant)
```

- **Schemas are files, versioned by their source** — git tags for published versions, branches for drafts, local filesystem for development. Loco doesn't reinvent version control.
- **Projects own namespaces** — `ben/crm` is a namespace. Tenants are instances that install it.
- **`loco/core` ships as a built-in namespace** — always installed, provides foundational collections (user, etc.).

### New User Flow

1. **Sign up** → get a username (`ben`)
2. **Create a project** → `ben/crm` — initialize a namespace folder (or git repo) with the standard structure and a default `dev` tenant
3. **Build your schema** → edit YAML files locally, or via MCP/CLI. With the local filesystem adapter, changes are picked up on reload.
4. **Create tenants** → add `acme`, `globex`, etc.
5. **Publish (optional)** → `git tag v0.0.1 && git push --tags`, or `npm publish`. Pin production tenants to the tag.

### Implementation

- [ ] **`SchemaSource` trait** — `load(uri) -> Vec<TypeDef>` with adapters for resolving namespace URIs to parsed schema files
- [ ] **Local filesystem adapter** — reads from a local path (`file://`). This is what exists today, just formalized behind the trait.
- [ ] **Runtime schema loading** — load schemas at server startup (and optionally reload on change) instead of only at build time
- [ ] **Project config file** — list of namespace source URIs + tenant configurations with pinned refs
- [ ] **Tenant namespace pinning** — each tenant specifies which namespaces and versions/refs it uses
- [ ] **`loco/core` as built-in** — bundled with loco-apps, always available
- [ ] **Validate records against schema at write time** — enforce field types, required fields (loose on load, strict on save)
- [ ] **Support additional field types** — date, datetime, reference (foreign key), picklist, multi-select, rich text, file/attachment
- [ ] **Git adapter (future)** — clone/fetch a repo, checkout a ref, load schemas from the working tree
- [ ] **npm adapter (future)** — download a package, extract, load schemas

## Phase 3: API Key Authentication

User-scoped API keys for authenticating requests. This is the minimum viable auth needed before exposing loco to external clients (MCP, CLI, etc.).

- [ ] **`api_key` collection in `loco/core`** — stores key hash, user reference, label, created/last-used timestamps, active flag
- [ ] **Key generation** — API endpoint or CLI command to create a key for a user; key is shown once at creation, only the hash is stored
- [ ] **Request authentication** — Axum middleware that resolves `Authorization: Bearer <key>` to a user + tenant; replaces or supplements the current `X-Tenant-Id` / `?tenant=` mechanism
- [ ] **Key → user → tenant resolution** — the API key identifies the user, the user belongs to a tenant, so tenant is derived automatically (no more manual tenant header for authenticated requests)
- [ ] **Multiple keys per user** — support labeling keys (e.g., "CLI", "CI pipeline", "MCP") and revoking individually
- [ ] **Key revocation** — deactivate a key without deleting it (for audit trail)
- [ ] **Rate limiting** — per-key request limits to prevent abuse

## Phase 4: MCP Server

Expose loco as an MCP (Model Context Protocol) server so AI agents can discover and interact with the platform natively.

- [ ] MCP tool: `create_collection` / `create_field` — define schemas conversationally
- [ ] MCP tool: `insert_record` / `update_record` / `delete_record` — full CRUD
- [ ] MCP tool: `query_records` — list, filter, sort, paginate
- [ ] MCP tool: `describe_schema` — inspect available collections and fields
- [ ] MCP tool: `generate_report` — summarize, aggregate, and analyze data
- [ ] MCP resource: expose collections and schemas as browsable resources
- [ ] Support connecting from Claude Desktop, Cursor, Claude Code, and other MCP clients

## Phase 5: Loco CLI

A command-line tool for working with loco data — useful for humans and scriptable by agents.

- [ ] `loco collections list` / `loco fields list` — inspect schemas
- [ ] `loco records list <collection>` — display data in ASCII tables
- [ ] `loco records add <collection>` — interactive or flag-based record creation
- [ ] `loco records export <collection> --format csv|json|yaml` — bulk export
- [ ] `loco records import <collection> <file>` — bulk import
- [ ] `loco tenants list` / `loco tenants create` — tenant management
- [ ] Configurable output formats (table, JSON, CSV, YAML) via `--output` flag
- [ ] Connection profiles (save server URL + tenant for quick switching)

## Phase 6: Security Model

Fine-grained, declarative security at every level.

### Authentication

Pluggable auth via an `AuthAdapter` trait — same adapter pattern as the data lake and file lake.

- [ ] **`AuthAdapter` trait** — `authenticate`, `validate_session`, `get_user`, `refresh_token`, `revoke_session`
- [ ] **Local auth adapter** — email/password stored in `loco/core.user` with bcrypt hashing. No external dependencies. Good for dev and self-hosted.
- [ ] **Clerk adapter** — delegate to Clerk for production use (MFA, social login, session management, hosted UI)
- [ ] **Future adapters** — Auth0, Supabase Auth, Keycloak, generic OIDC/OAuth2
- [ ] **API key auth** — for service-to-service and CLI access, scoped per tenant
- [ ] **Session middleware** — Axum middleware that resolves the current user from the auth adapter and attaches it to the request context
- [ ] **Configurable per tenant** — different tenants could use different auth providers

### Authorization

- [ ] **Role & permission system** — define roles in `loco/core`, assign to users, enforce everywhere
- [ ] **Collection-level security** — per-tenant permissions (read, write, delete) on entire collections
- [ ] **Record-level security** — ownership-based access, sharing rules, team/role visibility
- [ ] **Field-level security** — hide or make read-only specific fields per role (e.g., salary visible to HR only)
- [ ] **Row-level filtering** — queries automatically scoped by the caller's permissions (no data leaks by default)
- [ ] **Audit trail** — who changed what, when, on every record

## Phase 7: Hosted Platform

Offer loco as a managed service.

- [ ] Self-service tenant signup and onboarding
- [ ] Per-tenant resource isolation and usage limits
- [ ] Admin dashboard for tenant management (or CLI/MCP-based admin)
- [ ] Billing and usage metering
- [ ] Custom domains per tenant
- [ ] Managed database backends (Postgres, Turso, etc.)

## Phase 8: Namespace Marketplace

Since namespaces are just folders published to git or npm (see Phase 2), the marketplace is a discovery and curation layer on top of existing package infrastructure — not a custom registry.

- [ ] **`loco install <namespace>`** — resolve a namespace URI, add it to the project config, pull the schema files
- [ ] **Dependency resolution** — `loco.yaml` declares dependencies (e.g., `ben/crm` depends on `loco/core`), resolved transitively on install
- [ ] **Marketplace catalog** — a web directory (or CLI-searchable index) of published namespaces with descriptions, ratings, and install counts
- [ ] **Templates** — starter namespaces for common use cases: CRM, project management, inventory, HR, etc.
- [ ] **Upgrade paths** — `loco upgrade ben/crm@0.0.2` updates the pinned version, shows schema diff, flags breaking changes

## Phase 9: Scripting Engine

Sandboxed TypeScript runtime for user-defined logic. Users write code that runs in response to data events, scheduled jobs, or external triggers. Built on Deno's Rust crates (`deno_core` / `deno_runtime`) for secure, sandboxed execution with fine-grained permissions.

- [ ] **Record triggers** — execute TypeScript on record create, update, or delete (before and/or after)
- [ ] **Sandboxed execution** — each script runs in an isolated context with no filesystem/network access by default; grant permissions explicitly
- [ ] **Script API** — provide a `loco` global object in the sandbox with access to the current record, collection, tenant, and data lake operations (query other collections, insert records, etc.)
- [ ] **Script storage** — store scripts in the lake or as files associated with a collection/namespace
- [ ] **Error handling & logging** — capture script errors, console output, and execution metrics per run
- [ ] **Execution limits** — timeout, memory caps, and rate limiting to prevent runaway scripts

### Event & Task Engine

Extend the scripting engine into a full event-driven task system.

- [ ] **Task queue** — durable, ordered queue for async work (powered by SQLite or Postgres)
- [ ] **Cron schedules** — run scripts on a schedule (e.g., "every night, generate a summary report")
- [ ] **Webhook triggers** — inbound webhooks that trigger script execution with the payload as input
- [ ] **Event bus** — internal pub/sub for record changes, enabling fan-out to multiple scripts
- [ ] **Chained tasks** — one script's output feeds into the next (simple workflow/pipeline support)
- [ ] **Retry & dead-letter** — automatic retries with backoff, dead-letter queue for persistent failures
- [ ] **Execution history** — queryable log of all task runs with status, duration, and output

## Phase 10: File Lake

Adapter-based file storage — the same pattern as the data lake, but for unstructured files. The file lake doesn't store files itself; it provides a uniform interface over pluggable storage backends.

- [ ] **FileLakeAdapter trait** — `upload`, `download`, `delete`, `list`, `get_metadata`
- [ ] **Local filesystem adapter** — store files on disk (good for dev)
- [ ] **S3-compatible adapter** — AWS S3, MinIO, R2, GCS (via S3 compatibility)
- [ ] **Tenant-scoped storage** — files are isolated per tenant, just like data
- [ ] **File metadata in the data lake** — store file references (name, size, content type, storage path) as records, so files are queryable and linkable to other records
- [ ] **File field type** — a new field type for collections that references a file in the file lake
- [ ] **Presigned URLs** — generate time-limited download/upload URLs for direct client access
- [ ] **Size & type limits** — configurable per tenant or collection

## Phase 11: Client SDKs & Framework Integrations

Thin client libraries that make it trivial to build frontends against loco-apps using standard tools. The goal is not to build a UI framework — it's to provide the glue so that existing tools (React, Vite, TanStack Query, etc.) connect to loco with minimal boilerplate.

### Core: `@loco/client` (TypeScript)

A framework-agnostic TypeScript client that all framework integrations build on.

- [ ] **Typed client** — generated or hand-written client with methods for all CRUD operations, tenant/namespace scoped
- [ ] **Pagination** — cursor-based and offset pagination built into `list` queries
- [ ] **Filtering & sorting** — pass filter/sort params that map to server-side query capabilities
- [ ] **Batch operations** — batch multiple reads or writes into a single request
- [ ] **Error handling** — structured error types matching the server's `ApiResponse` format
- [ ] **Auth integration** — attach tenant ID, API keys, or tokens automatically per request
- [ ] **File lake support** — upload/download helpers, presigned URL handling

### React: `@loco/react`

Hooks that wrap `@loco/client` with TanStack Query (React Query) for caching, deduplication, and background refetching.

- [ ] **`useLoco(collection)`** — returns a configured client scoped to a collection
- [ ] **`useLocoQuery(collection, filters?)`** — list records with automatic caching, pagination, and refetching
- [ ] **`useLocoRecord(collection, id)`** — fetch a single record with cache
- [ ] **`useLocoMutation(collection)`** — insert/update/delete with optimistic updates and cache invalidation
- [ ] **`useLocoSchema(collection?)`** — inspect available collections and fields at runtime
- [ ] **Built on TanStack Query** — no custom cache layer; users get devtools, deduplication, stale-while-revalidate, and background refetching for free
- [ ] **SSR-friendly** — works with Next.js, Remix, and other server-rendering frameworks

### Realtime

Push data changes to connected clients so UIs stay in sync without polling.

- [ ] **Server-Sent Events (SSE)** — lightweight, HTTP-based, works through proxies and load balancers. Good default for most use cases.
- [ ] **WebSocket upgrade path** — for bidirectional communication when SSE isn't enough (collaborative editing, presence)
- [ ] **Subscription API** — `subscribe(collection, filters?)` returns a stream of change events (insert, update, delete)
- [ ] **TanStack Query integration** — realtime events automatically invalidate or update the query cache, so `useLocoQuery` results update live
- [ ] **Tenant-scoped channels** — realtime events are isolated per tenant, same as data

### Future Framework Support

- [ ] **Vue** — `@loco/vue` composables (`useLocoQuery`, etc.) wrapping TanStack Query for Vue
- [ ] **Svelte** — `@loco/svelte` stores with TanStack Query integration
- [ ] **Vanilla JS** — `@loco/client` works standalone for non-framework use cases
- [ ] **Mobile (React Native)** — `@loco/react` should work out of the box; test and document

## Phase 12: Advanced Data Features

As the platform matures, add capabilities that make it competitive with established tools.

- [ ] **Relationships** — explicit foreign key references between collections, with cascading deletes and referential integrity
- [ ] **Computed fields** — fields whose values are derived from expressions or other fields
- [ ] **Validation rules** — custom constraints beyond type checking (regex, ranges, uniqueness, required-if)
- [ ] **Full-text search** — search across records and fields with ranking
- [ ] **Bulk operations** — batch insert, update, delete with transactional guarantees
- [ ] **Change data capture** — stream record changes to external systems
- [ ] **Webhooks** — notify external services on data events

---

## Ideas & Open Questions

- **Agent SDK** — a higher-level library that wraps the MCP tools and provides a convenient API for building agents that work with loco data. Could be in Python, TypeScript, or Rust.
- **Formula language** — a simple expression language for computed fields, validation rules, and filters (similar to Salesforce formulas or Excel expressions).
- **Multi-adapter per tenant** — different tenants could use different storage backends (SQLite for dev, Postgres for production).
- **Snapshot & restore** — export an entire tenant's data and schemas for backup, migration, or cloning.
- **GraphQL or query DSL** — a richer query interface beyond simple list/get, allowing joins across collections, aggregations, and nested queries.
