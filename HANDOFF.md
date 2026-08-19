# Handoff — 2026-08-18

For the next session. Read `README.md` + `CLAUDE.md` + [`docs/identity.md`](docs/identity.md) if you are cold. This file is only “where we left off” and what to do next.

## This session

Architecture only. No code.

- Locked the target identity model: global Loco identities, person/org accounts, project `developer`/`editor`, no application-user table. Doc: [`docs/identity.md`](docs/identity.md).
- Agreed standalone frontends (CloudFront, no secrets) and a later Clippy, but those wait until this stack exists.
- Field-metadata / schema→form loop (old HANDOFF §1) is still valid work and orthogonal. If one person: park it until after PR 2 so Hurl/Studio do not rebase twice. Do not mix it with PR 2.

## Servers

```bash
cargo run -p loco-apps          # :3000
npm run dev -w loco-studio      # :5174  (proxies /api → :3000)
```

Login is still username-only against site-scoped users under `loco-apps/auth/` (gitignored). That is what PR 1 deletes.

`ben/pets` is gitignored scratch. `loco/core`, `loco/studio`, `loco/cards` are committed.

## Next — identity PR stack

Hard cut, not a migration saga. Each PR leaves `cargo test` green. Do not start MCP, CLI, Clippy, or anything in `FUTURE_IDEAS.md` until PR 2 (membership) is trustworthy. CORS / a cross-origin page is PR 5, not a side quest.

### PR 1 — Global identity (no policy change)

You log into Loco, not into a site. Authorization *behavior* stays the same so existing Hurl still passes.

- `Account` (`handle`, `type: person | org`) + `Identity` (login, 1:1 with a person account).
- Sessions and API keys hang off the identity. Stop storing them under `{user}/{project}/{site}/`.
- `POST /auth/login` does not need `X-Site-Id` to find the user. `SiteScope` headers remain for the rest of the request.
- Seed fixtures: `alice` and `bob` are person accounts; `loco` is an org. Person-owned `{handle}/*` still implies owner via the old string-match rule (temporary).
- Password on the adapter. Tests get a known password (or a test-only bypass you remove in PR 2).
- Studio: login once; still send `loco/studio` / `loco/cards` headers so today’s gates keep working.

**Touch:** `loco-apps/src/auth/` (`mod.rs`, `local.rs`), `handlers/auth.rs`, Hurl suites that `POST /auth/login` (especially `tests/suites/authorization/`), Studio `src/api.js` / `src/auth.js` only if the login body changes.

**Done when:** `cargo test` is green, Studio login still works, `AuthUser.site_id` is no longer who you are.

### PR 2 — Membership replaces the three gates

The real architecture PR. Do not split it.

Delete `METADATA_EDITOR_SITES`, `require_can_edit_user`, and “this *site* may edit schema.”

Add `org_members (org, identity, owner|member)` and `project_members (project, identity, developer|editor)`. Authz on `SiteScope`: token → identity → union of org role + project role.

- `/schema` + `/config` writes: `developer` or org `owner`, plus draft version.
- `/data` writes: `developer` or `editor` (public hole stays until PR 3).
- `/config` invite + list/remove members. Pending identity if the handle does not exist yet.
- Create org → creator is org `owner`. Create person-owned project → creator is project `developer`.
- `loco` is an org. Editors of `loco/studio` are members, not `username == loco`.

Rewrite `loco-apps/tests/suites/authorization/authorization.hurl`:

1. Alice (owner of `alice/testapp`) can mutate schema.
2. Bob cannot.
3. Alice invites Bob as `editor` → Bob can `/data`, cannot `/schema`.
4. Promote Bob to `developer` → Bob can `/schema`.
5. Org path: create `acme`, Alice is owner, Alice creates `acme/crm` and edits its schema.

Studio: token is the person; `X-Project-Id` is the project; `X-Site-Id` is the pin. No more “log into the magic editor site.”

**Touch:** `http/scope/site.rs`, `version.rs`, `config.rs`, `http/scope/mod.rs`, `handlers/config.rs`, `handlers/auth.rs`, authorization fixtures + Hurl, Studio project/session header usage.

**Done when:** that Hurl file is the spec and the hardcoded allowlist is gone.

### PR 3 — Public is a policy, not a hole

- Site or collection flags: `publicRead` / `publicCreate`. Default: **no** public write or delete.
- Anonymous `/data` is principal `public`. List/get only if `publicRead`. Insert only if `publicCreate`. Update/delete never (until RLS).
- Flip `tests/suites/authorization/data_no_auth.hurl` — it currently asserts public can add and delete.

**Done when:** no token cannot delete, and a fixture can opt a collection into public create.

### PR 4 — Schema read vs schema write

- `GET /schema/...` for `developer`, `editor`, and `public` on a public-read site (pinned version only).
- Writes stay `developer` + draft.
- Split the read extractor from `VersionScope` (today every `/schema` route requires auth + editor site).

**Done when:** a token-less (or editor) client can list fields for a public site’s pinned version.

### PR 5 — Prove the standalone frontend

CORS on the API. A 40-line page on another origin that lists a public collection. Leave Studio’s Vite proxy alone. The page is the dogfood, not a product.

**Done when:** that page works with only `{ apiUrl, projectId, siteId }` in the source and no token.

## After this stack

Field metadata (`description` / `required` / `variant` on `field.yaml`, validator, Studio forms) — old HANDOFF §1. Then `@loco/client`, MCP, CLI, Clippy. See `FUTURE_IDEAS.md`.

## Conventions that still apply

- Template vars on types must be `slug` + `createOnly`. Instance YAML bodies do not repeat them.
- Draft versions contain `-` (`0.0.1-dev`); only drafts accept `/schema` writes.
- Lake is schemaless; loco-apps is strict on write, warnings on read.
- Data requests: `X-Project-Id: {account}/{project}`, `X-Site-Id: {site}`. First segment is an account handle, not an ACL (after PR 2).
