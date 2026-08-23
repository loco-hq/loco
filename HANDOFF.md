# Handoff — 2026-08-22

For the next session. Read `README.md` + `CLAUDE.md` + [`docs/identity.md`](docs/identity.md) if you are cold. This file is only “where we left off” and what to do next.

## Last session

Studio production build (PR 1 of the Node-free Studio design). Dropped the `/api` Vite rewrite. `api.js` talks to `/auth` `/config` `/schema` `/data`. API origin is the `API_ORIGIN` constant in `loco-studio/src/config.js` (default `http://localhost:3000`). Dev still uses a passthrough Vite proxy of those four prefixes. `npm run build -w loco-studio` emits a static `dist/` that can be served with `python3 -m http.server` — no Node at runtime.

Identity stack (PRs 1–5) is done. Next is the field-metadata / schema→form loop. Serving `dist/` from Axum (`cargo run` = API + Studio) is the leftover Studio follow-up, not blocking schema→form.

## Servers

```bash
cargo run -p loco-apps          # :3000
npm run dev -w loco-studio      # :5174  (proxies /auth /config /schema /data → :3000)
npm run build -w loco-studio && python3 -m http.server 5174 --directory loco-studio/dist
                                # static Studio → :3000 via API_ORIGIN
python3 -m http.server 5176 --directory examples/public-page
                                # :5176  (static page → :3000, CORS)
```

Login is global (`{ username, password }`). Seeded people (`alice`, `bob`) use password `password`. Membership (PR 2) is the ACL.

`ben/pets` is gitignored scratch. `loco/core`, `loco/studio`, `loco/cards`, `loco/demo` are committed.

## Identity PR stack — done

### PR 1 — Global identity — done

### PR 2 — Membership replaces the three gates — done

### PR 3 — Public is a policy, not a hole — done

Anonymous `/data` is principal `public`. Grants are permission sets assigned on the site as `public_permission_sets`. Spec: `tests/suites/authorization/data_no_auth.hurl`.

### PR 4 — Schema read vs schema write — done

`GET /schema` uses `VersionReadScope`: developer/editor any version (no site headers); `public` (and authenticated non-members) on a site that assigns at least one permission set, pinned version only. Writes stay `VersionScope` (developer + draft). Spec: `tests/suites/authorization/schema_read.hurl`.

### PR 5 — Prove the standalone frontend — done

CORS on the API (`*` origin / method / header, no cookies). `examples/public-page/` is a static page on `:5176` that lists `loco/demo` guestbook with `{ apiUrl, projectId, siteId }` and no token. Spec: `tests/suites/authorization/cors.hurl`.

## Studio static SPA

PR 1 (client origin, drop `/api`) is this session. PR 2 (Axum `ServeDir` of `loco-studio/dist`) is not done. Field-metadata remains next for product work.

## After this stack

Field metadata (`description` / `required` / `variant` on `field.yaml`, validator, Studio forms) — old HANDOFF §1. Then `@loco/client`, MCP, CLI, Clippy. See `FUTURE_IDEAS.md`.

## Conventions that still apply

- Template vars on types must be `slug` + `createOnly`. Instance YAML bodies do not repeat them.
- Inline object `name:` is snake_case (`collection_grant`); codegen PascalCases it.
- Draft versions contain `-` (`0.0.1-dev`); only drafts accept `/schema` writes.
- Lake is schemaless; loco-apps is strict on write, warnings on read.
- Data requests: `X-Project-Id: {account}/{project}`, `X-Site-Id: {site}`. First segment is an account handle, not an ACL.
