# Roadmap

Loco is a low-code backend for structured data — think Salesforce or Airtable, but aimed at AI agents and tools rather than drag-and-drop UIs.

**Backend-first.** Schema enforcement, versioned metadata, and a reliable API are the product. Studio is a convenient editor, not the interface the platform is built around.

**Loose on load, strict on save.** The lake is schemaless. Reads tolerate missing fields, extra fields, and type drift (surfaced as diagnostics). Writes must match the current schema.

## Built

- YAML type definitions → Rust structs at build time (`loco-gen-schema`)
- Runtime instance loading + YAML persistence (`loco-schema-runtime`)
- Versioned projects: manifests, draft versions (`*-dev`), collections, fields, fieldsets
- Direct-dependency visibility via `VersionSchema` (not transitive)
- Sites + datasets instead of a tenant registry
- REST API: `/data`, `/schema`, `/config`, `/auth`
- Pluggable lake (`sqlite`, `memory`) scoped by `dataset_id`
- Write-time validation (unknown fields + scalar types) and read-time diagnostics
- `AuthAdapter` + local filesystem adapter: sessions, users, API keys
- Username-only login; metadata-editor sites are a hardcoded allowlist
- Studio for schema + records; `loco-ui` field primitives
- Hurl suites covering auth, CRUD, validation, and project/version/schema lifecycle

## Next

In order. Do not start later items until the one above is something you can trust.

1. **Finish the schema → form loop.** Collection fields need `description`, `required`, and maybe `variant`. Validate required fields on create/update. Drive record forms (and keep tables) from the `auto_add` fieldset. Register `SelectField` on the dispatcher. Stop offering `list` as a record field type until the validator and a control exist.

2. **Make auth real enough to build on.** Password on login (the adapter already accepts one). Replace the hardcoded editor-site list with something declared — even a simple "this site may edit metadata" flag. Then API keys are actually useful for a CLI or MCP.

3. **Then interfaces.** MCP tools and a `loco` CLI on top of the same API. Not before (1) and (2).

## Later

Parked, not scheduled. Detail lives in [`FUTURE_IDEAS.md`](FUTURE_IDEAS.md) — MCP tool list, CLI surface, extra auth adapters, permissions, git/npm schema sources, marketplace, scripting, file lake, SDKs, realtime, hosted platform.

The short version:

- Richer field types: date, reference, picklist, file
- Query: filter, sort, paginate
- Relationships and referential integrity
- Roles / collection- and field-level permissions
- Schema sources beyond the local filesystem (`git://`, `npm://`) and a namespace marketplace
- Hosted platform
- Scripting / triggers / task queue
- File lake
- Typed client SDKs and realtime
