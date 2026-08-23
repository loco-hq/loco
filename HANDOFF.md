# Handoff — 2026-08-22

For the next session. Read `README.md` + `CLAUDE.md` + [`docs/identity.md`](docs/identity.md) if you are cold. This file is only “where we left off” and what to do next.

## Last session

PR 4 — schema read vs schema write. `cargo test` is green.

- GET `/schema` is `VersionReadScope` (read-only). Writes stay `VersionScope` (developer + draft).
- Developer/editor: any version of a project they belong to, no site headers.
- `public` (and authenticated non-members): site headers required; path version must match the site pin; the site must assign at least one permission set to `public`. Whole pinned version, no per-collection filter.
- Spec: `tests/suites/authorization/schema_read.hurl`.

Field-metadata / schema→form loop is still valid work and orthogonal. Park it until after this identity stack.

## Servers

```bash
cargo run -p loco-apps          # :3000
npm run dev -w loco-studio      # :5174  (proxies /api → :3000)
```

Login is global (`{ username, password }`). Seeded people (`alice`, `bob`) use password `password`. Membership (PR 2) is the ACL.

`ben/pets` is gitignored scratch. `loco/core`, `loco/studio`, `loco/cards` are committed.

## Next — identity PR stack

Hard cut, not a migration saga. Each PR leaves `cargo test` green. Do not start MCP, CLI, Clippy, or anything in `FUTURE_IDEAS.md` until this stack is trustworthy. CORS / a cross-origin page is PR 5, not a side quest.

### PR 1 — Global identity — done

### PR 2 — Membership replaces the three gates — done

### PR 3 — Public is a policy, not a hole — done

Anonymous `/data` is principal `public`. Grants are permission sets assigned on the site as `public_permission_sets`. Spec: `tests/suites/authorization/data_no_auth.hurl`.

### PR 4 — Schema read vs schema write — done

`GET /schema` uses `VersionReadScope`: developer/editor any version (no site headers); `public` (and authenticated non-members) on a site that assigns at least one permission set, pinned version only. Writes stay `VersionScope` (developer + draft). Spec: `tests/suites/authorization/schema_read.hurl`.

### PR 5 — Prove the standalone frontend

CORS on the API. A 40-line page on another origin that lists a public collection. Leave Studio’s Vite proxy alone. The page is the dogfood, not a product.

**Done when:** that page works with only `{ apiUrl, projectId, siteId }` in the source and no token.

## After this stack

Field metadata (`description` / `required` / `variant` on `field.yaml`, validator, Studio forms) — old HANDOFF §1. Then `@loco/client`, MCP, CLI, Clippy. See `FUTURE_IDEAS.md`.

## Conventions that still apply

- Template vars on types must be `slug` + `createOnly`. Instance YAML bodies do not repeat them.
- Inline object `name:` is snake_case (`collection_grant`); codegen PascalCases it.
- Draft versions contain `-` (`0.0.1-dev`); only drafts accept `/schema` writes.
- Lake is schemaless; loco-apps is strict on write, warnings on read.
- Data requests: `X-Project-Id: {account}/{project}`, `X-Site-Id: {site}`. First segment is an account handle, not an ACL.
