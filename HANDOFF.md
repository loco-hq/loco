# Handoff — 2026-08-22

For the next session. Read `README.md` + `CLAUDE.md` + [`docs/identity.md`](docs/identity.md) if you are cold. This file is only “where we left off” and what to do next.

## Last session

Permission-set reshape (after PR 3, before PR 4). `cargo test` is green.

- Grants are `collections: [{ collection, read, create, update, delete }]`. Unspecified verbs are false. Public may do any verb an assigned set grants.
- Bare collection names match by name (`VersionSchema` prefers self). Qualified `{project}.{name}` (e.g. `ben/crm.contacts`) is accepted and pins the owner.
- `list` items may be inline objects (`type: object`, `name: collection_grant` → generated `CollectionGrant`). Nested lists still rejected.
- Spec: `tests/suites/authorization/data_no_auth.hurl`. Guestbook on `dev` is stacked read+create; `wiki` on `open` is all four verbs.

Field-metadata / schema→form loop is still valid work and orthogonal. Park it until after this identity stack. Do not mix it with PR 4.

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

### PR 4 — Schema read vs schema write

Today every `/schema` route uses `VersionScope`: authenticated + `require_developer` on the path project. No site headers. Editors get 403 on GET. Public cannot read schema at all.

- `GET /schema/...` for `developer`, `editor`, and `public` on a site that assigns at least one permission set to `public` (pinned version only).
- Writes stay `developer` + draft (`VersionScope` as it is).
- Split a read extractor from `VersionScope`. Public reads need site headers so you can check the pin (`X-Project-Id` + `X-Site-Id`); path `{version}` must match the site's `version`. Developers/editors can keep reading any version of a project they belong to (no site headers required).
- Do not filter the schema body per collection in this PR — any assigned set → whole pinned version. Per-collection schema visibility is a later question.

**Touch:** `loco-apps/src/http/scope/version.rs` (and a sibling read extractor), `handlers/schema.rs` GET vs POST/PUT/DELETE, `docs/identity.md` item 7, a Hurl spec (token-less GET fields on `alice/testapp` + `open` or `dev`; editor GET; public GET of a non-pinned version 403/404; writes still 401/403).

**Done when:** a token-less (or editor) client can list fields for a public site’s pinned version.

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
