# Domains

Target model. Nothing here is implemented yet; the implementation issues are at the end. This replaces "Sites as hosts" in [`hosting.md`](hosting.md), `LOCO_DEFAULT_SITE`, and issue #145, and amends [`identity.md`](identity.md) on where login happens.

A site is reached by name. Every hostname the server answers — the apex included — resolves through one table of names, and those names live on the sites that own them. A Loco user logs in once, on the platform host. A hosted app uses that login only if its project lists the app's origin.

## Vocabulary

| Term | What it is |
|---|---|
| **Platform domain** | The domain this process serves, from `LOCO_DOMAIN` (`lococo.run` in production, `localhost` in development). |
| **Domain name** | A label on a site: `benblog` is `benblog.{platform domain}`. `@` is the platform domain itself. |
| **Primary** | The one domain name a site's links, the discovery document, and Studio use. |
| **Platform host** | `api.{platform domain}`. Where login happens and where tools call. Its API is never pinned to a site. Its pages are the bundle of whichever site claims the name `api` (Studio, today). |
| **Site origin** | `https://{name}.{platform domain}` for any domain name on a site. |

## Names live on the site

```yaml
# schemas/instances/ben/blog/sites/www.yaml
label: Public blog
version: 0.0.1
dataset: prod
domains:
  - name: unicorn-particle-horse
  - name: benblog
    primary: true
```

`domains` is an inline list of `site_domain` objects (`name`, `primary`) on the `site` type (`loco-apps/schemas/types/site.yaml`). There is no separate domain document and no top-level `domains/` directory in the store, which would collide with an account of that name.

- **A name is a label, not a host.** The host is `{name}.{platform domain}`, or the platform domain for `@`. The same store serves `benblog.localhost:3000` in development and `benblog.lococo.run` in production without rewriting.
- **Charset:** a host label — `[a-z0-9-]`, 1–63, not starting or ending with `-` — or the single token `@`. Uppercase is 400, not folded. This is a new `check_domain` in `http/names.rs`: `check_slug` is the wrong rule (it allows `_` and rejects `-`). In `site.yaml` the property is `type: string`, because codegen's `slug` rejects `@`.
- **Unique across the server.** `ProjectConfig::create_site` and `update_site` (`http/project_config.rs`), which already hold `PINS`, check every other site for the name and answer 409 naming that site's project and site. `delete_site` takes `PINS` too, so the check cannot race a delete; project delete already holds it. The host resolver reads the same site store, so there is no separate index to go stale. Two sites on disk claiming one name is a boot warning naming both; neither resolves until one is removed.
- **Writes.** `domains` is part of the site body. On create it is optional. On update (`SiteUpdate`, a patch) a body that names `domains` replaces the list and is checked; a body that omits it leaves the names alone and is not re-checked.
- **Exactly one primary.** On write, more than one `primary: true` is 400. None marked means the first entry is primary.
- **Random default.** Every site create — including the `dev` site that `create_project` bootstraps through `create_site` — assigns one random name (three words from a 50-word list compiled into the binary, `unicorn-particle-horse` — 125,000 names, enough for now and easy to grow — retried on collision) and stores it in `domains`. Random names are not secrets. It is an ordinary entry: it does not change on restart, and it can be removed once the site has another name.
- **Delete releases.** Deleting a site — or its project — releases every name on it. To keep a name, move it to another site first (remove it here, add it there, two writes under `PINS`).
- **Who may claim:** whoever may write the site (developer or org owner of its project). There is no per-account limit yet; a limit, or claimable names as a paid feature with random names on the free plan, comes later.

### Reserved names are seed records

The server owner claims names by committing them on seed sites, the same way seed projects are committed (`schemas/seed/`, copied into the store at boot when absent, never overwritten — `loco-apps/src/seed.rs`). They are claimed before any user exists.

`loco/studio/sites/studio.yaml` carries `domains: [{name: api}]`. That is how Studio becomes the platform host's pages, and the only thing that makes it so: Studio is an ordinary bundle on an ordinary site. Replacing Studio is changing which seed site claims `api`.

Seeds are never re-synced, so a store that already has `loco/studio` does not pick up the new line. On an existing store, delete `loco/studio` from `schemas/instances/` and restart, as for any changed seed.

`@` is unclaimed and serves nothing for now.

### The one special name

`api` is the only name the server treats differently, and the difference is about the host, not the site that claims it:

- Its API is **never pinned**: `/data` and `/actions` take their site from `X-Project-Id` / `X-Site-Id`, as today's apex does. This is the API for anything that works across projects — Studio, the CLI, MCP.
- It is the **full-access origin** for a session (see "Access from a hosted app").
- Non-API paths serve the claiming site's pinned bundle, as any site host does.
- Only a seed may claim it: a `/config` write that would **add** `api` to a site that does not already hold it is 400. A write that round-trips the claiming site's own list is fine.
- Developers of the project that holds `api` ship the code that runs as the full-access origin. That is the same trust Studio has today, accepted on purpose.

### Custom domains, later

A custom domain will be another kind of entry in the same `domains` list — a name containing a dot (`inventory.example.com`) with verification state (a DNS TXT record the owner sets) and certificate status — and another match arm in the resolver. This doc does not specify them.

## Resolving a request

`http/host.rs` `resolve_site` becomes one lookup:

1. Take `Host` (or the URI authority, as today, for HTTP/2 without a `Host` header), drop the port, lowercase it, trim a trailing dot.
2. Equal to the platform domain → name `@`. One label followed by `.{platform domain}` → that label. Anything else → no site.
3. `api`: the bundle is the claiming site's, and `RequestSite.from_host` is false, so headers are not pinned (step 5).
4. Any other name held by a site is that site, with `from_host` true: absent `X-Project-Id` / `X-Site-Id` are filled in, and a header naming another site is 400 — as a subdomain is today, with no exception.
5. Otherwise the API answers with `/data` and `/actions` taking their site from the headers, and `/` is the JSON 404 with `see: /.well-known/loco.json`.

`site_ref_from_host` (the three-label parse), `parse_site_ref`, `Config::default_site`, and `AppState::default_site` are deleted, with `LOCO_DEFAULT_SITE`. A three-label link stops resolving.

`LOCO_DOMAIN` is a `Config` field (`loco-apps/src/config.rs`), default `localhost`. Boot logs the platform domain, the platform host, and one line per seed-claimed name.

## Access from a hosted app

identity.md's model stands: only Loco users log in, access is membership, and headers select a site. What changes is **where** the login is typed and **which pages may use it**. This is Sanity's model: one universal login; each project lists the origins allowed to act with it ([Sanity: CORS and browser security](https://www.sanity.io/docs/content-lake/browser-security-and-cors)).

### One session, on the platform host

- **Where password login and signup answer** (`POST /auth/login`, `POST /auth/users`), by host:

  | Host | Answer |
  |---|---|
  | The platform host | Sets the session cookie **and** returns the JSON token as today |
  | A host that names no site (the apex, `127.0.0.1`) | Returns the JSON token, no cookie. This is how tools, the Hurl runner, and Studio-in-Vite's proxy log in today, and it keeps working |
  | A site host other than `api` | 404 with `see`. A hosted app cannot collect a password |

- **The session cookie:** `HttpOnly`, `Secure` (dropped when the request is not TLS), `SameSite=Lax`, host-only (no `Domain` attribute), `Path=/`, `Max-Age` 7 days, matching `SESSION_TTL_DAYS`. The server still expires the session itself. `POST /auth/logout` clears the cookie.
- **Reading it:** `auth::session_or_public` reads the cookie beside `Authorization`. When both are present, the bearer wins, so a tool's header is never silently replaced by a browser cookie. A request with neither is `public`, as today.
- **What HttpOnly does and does not buy.** Script on a site origin cannot read the cookie, and its credentialed calls are confined to its own site (below). Script on the platform host can read the JSON token that login returns, so the platform host's bundle is trusted (see "The one special name"). The token stays in the login response on purpose: tools depend on it. Do not remove it.
- **The login and signup pages are Studio's.** Studio is the platform host's bundle, so its login screen posts to its own origin and the cookie lands on the platform host. Logging into Studio is the global login. Studio gains a signup screen.
- **Returning to an app.** An app sends the user to `https://api.{platform domain}/?return={url}`. Studio logs them in, or, if they already are, continues at once with no confirmation step, by navigating to `GET /auth/continue?return={url}`. Studio learns "already logged in" from `GET /auth/me`; it cannot see the cookie. The server parses `return` as a URL and answers 302 only when its scheme, host, and port equal the platform host's or a current site origin's; any path and query are kept. Userinfo, a backslash in the authority, and anything that does not parse are 400. The check is on the server, so the login page cannot bounce a user to an arbitrary site, whatever bundle claims `api`.
- **Any site may be a `return` target, on purpose.** An attacker who owns a site can receive the bounce, but the cookie is host-only on the platform host, so their page never sees it, and their origin can use the session only for their own site. There is no `state` parameter; none is needed.
- **Bearer tokens stay for tools.** The CLI, MCP, CI, and Studio-in-Vite keep `Authorization: Bearer` with a session token or an API key, as today. A bearer request is not subject to the origin check below; a browser page never holds one.

### Each project lists its origins

A **credentialed browser request** carries the session cookie and an `Origin` header. Its allowed origins are computed, not configured:

- the platform host (`https://api.{platform domain}`), which may act on any project the user can access, as a bearer token does today;
- every site origin of the project the request targets. A request from one of those origins acts **on that site only**: its `/data` and `/actions`, plus `/auth/me` and `POST /auth/logout`. The site is the one the origin's name resolves to; a header naming another site is 400. `/config` and `/schema` from a site origin are 403.

Anything else is 403 `origin {origin} may not use this session for {project}`. The check is on the server, in the request extractors (`http/scope/`), against the request's `Origin`, on cookie-authenticated `/data`, `/actions`, `/config`, `/schema`, `/auth/me`, and logout. It does not apply to `GET /auth/continue`: that is a top-level navigation, which often carries no `Origin`, and the `return` check is its control.

`Origin` is enforceable only because browsers set it. A non-browser client holding a stolen cookie can send any `Origin`, including the platform host's, and act on every project the user can reach — the same as a stolen bearer token. The origin check is the browser boundary, not a second factor.

**Removing a domain** removes that origin from the list on the next request. Sessions are not bound to names and are not revoked. If another site later claims the name, the origin resolves to the new site only, and membership still decides access.

**Adding someone else's project to your list is not possible.** The list is computed from the target project's own sites, and writing a site's `domains` needs developer or org owner on that project. A name on your site is an allowed origin of your project only. `api`, the one full-access origin, is seed-only.

So Bob, an editor on `alice/inventory`, opens `aliceapp.lococo.run`. The page calls `https://api.lococo.run/data/...` with `credentials: 'include'`. The browser sends Bob's cookie and `Origin: https://aliceapp.lococo.run`. The server resolves that origin to `alice/inventory`'s site, checks Bob's membership, and serves that site's data. The same page calling for one of Bob's own projects is 403: its origin is not on that project's list.

### CORS

`cors_layer` in `server.rs` decides by `Origin`, not by whether a cookie came along — a credentialed **preflight** carries `Origin` but no cookie, and answering it with `*` makes the browser drop the real call:

- `Origin` is the platform host or any current site origin → echo it in `Access-Control-Allow-Origin` with `Access-Control-Allow-Credentials: true` and `Vary: Origin`, on the preflight and on the request. Whether the request may act on a given project is the extractor's call above, not CORS's.
- Any other origin, or none → `Access-Control-Allow-Origin: *`, no credentials. Public and bearer calls behave as today.

### Forged requests

Every `*.lococo.run` page is the same site to the browser, so `SameSite=Lax` sends the cookie on a POST from `evil.lococo.run` to `api.lococo.run`. A form POST is not preflighted, so the two server rules — run on the actual request — are the CSRF boundary:

- A cookie-authenticated request must carry an `Origin` the target project allows (above). A same-site page that is not on the list is 403.
- A cookie-authenticated write must be `Content-Type: application/json` (parameters allowed). A plain HTML form cannot send that. Bearer and Hurl bodies are unaffected.

`lococo.run` is **not** added to the Public Suffix List. Doing so would make `api.lococo.run` and `aliceapp.lococo.run` different sites, turn the session into a third-party cookie, and break it in browsers that block those.

### Development

Checked in Chromium (2026-10-09), with a host-only `HttpOnly; SameSite=Lax` cookie set by the API host and a `fetch(…, {credentials: 'include'})` from an app host:

| API host → app host | Cookie sent |
|---|---|
| `api.localhost` → `app.localhost` | **No.** `localhost` is not a registrable domain, so every `*.localhost` name is its own site and the cookie is cross-site. |
| `api.localtest.me` → `app.localtest.me` | Yes, with `Origin: http://app.localtest.me`. A registrable domain makes its subdomains one site — the `lococo.run` case. |

So:

- `LOCO_DOMAIN` defaults to `localhost`. Everything but the cookie flow works there, offline: hosting by name, the platform host, Studio, bearer tokens. Hurl suites run here; they send cookies themselves and are not subject to the browser's rule.
- To exercise the cookie flow in a browser locally, run with `LOCO_DOMAIN=localtest.me` (a public wildcard that resolves to `127.0.0.1`; needs DNS). Cookies drop `Secure` when the request is not TLS.
- Studio-in-Vite and local example apps keep bearer tokens through their proxies (login on a host with no site) and are not affected.

## The API on every host

Unchanged: every host answers the whole API, so a hosted page's token-less calls work on its own origin. On a site's own host, `/data` and `/actions` are pinned to that site. A **logged-in** page calls the platform host instead, because that is where the cookie is sent.

## What the discovery document says

`src/discovery.json` and `discovery.rs` stop describing three-label hosts and `LOCO_DEFAULT_SITE`:

- A site's URL is its primary domain. Steps that create a site read `data.domains` from the response.
- URLs use `https` when the request arrived over TLS. The proxy (#107) is the only writer of `X-Forwarded-Proto`; nothing reads it today. URLs keep the request's port, as `request_listen_host` does now, and fall back to the literal `{platform domain}` when `Host` is not a valid hostname.
- Later, with the cookie: signup and login point at the platform host, and the guide's page links to `https://api.{platform domain}/?return=…` and calls the platform host with `credentials: 'include'`, replacing the #163 page's own login form. That rewrite belongs to the last implementation issue, not the first discovery change.

`loco-client` gains a cookie mode (`credentials: 'include'`, no token kept) beside today's bearer mode.

## What else changes

- **`docs/hosting.md`** — "Sites as hosts" is replaced by a pointer here; the apex section and `LOCO_DEFAULT_SITE` go.
- **`docs/identity.md`** — "Login does not take a site" stays true, and gains: login happens on the platform host; a site origin may use the session only for its own site.
- **CLAUDE.md** — Site hosting, `/auth`, and the CORS line.
- **Studio** — served at the platform host by claiming `api` on its seed site. Its API origin stays `''` (same origin). It gains signup and the `return` hop. Nothing in the server names Studio.
- **#163's guide** — hosted pages no longer log in on their own host. The guide's page links to the platform host and calls it with the cookie.
- **#107** — one proxied `*.lococo.run` record plus `lococo.run` covers every name, `api` included. The proxy sets `X-Forwarded-Proto`.

## Decided (Ben, 2026-10-09)

- Domains live on the site; a name another site holds is an error.
- Labels plus `@`; `LOCO_DOMAIN` names the platform domain.
- Subdomains only for now; custom domains later, same list.
- Reserved names are seed claims. `@` serves nothing for now.
- Studio stays and is not special: it is the bundle of the seed site that claims `api`. Logging into Studio is the global login.
- Sanity-style access: one HttpOnly session on the platform host; each project's allowed origins are its site domains plus the platform host; site origins act on their own site only.
- Hosted pages no longer log in on their own host.
- Random default names from a 50-word list.
- Only builders (Loco users) for now. Site users — an app's own accounts — are a separate design (identity.md "later").

- The cookie session lasts 7 days with no refresh, like today's sessions.
- Arriving at Studio with `?return=` while already logged in continues straight to the app, with no confirmation step.

## Open for Ben

1. **Deleting a site releases its names**, claimed ones included (no 409). Issue #162 proposed the opposite: a claimed name blocks site delete, like a pinned version. Releasing is simpler; a name you care about, you move first. Confirm.

## Implementation issues

Each is one PR, in this order; each depends on the one before. The orchestrator files them, replacing #145.

1. **Domains on sites.** `site_domain` list on `site.yaml` (`type: string`), `check_domain`, the 409 in `create_site` / `update_site` under `PINS`, `delete_site` taking `PINS`, the patch rule (named replaces, omitted keeps), one primary, a random default on every create (50 words), release on delete, `api` refused when it would be added. Hosts stay three-label, so nothing user-visible changes yet. Acceptance: Hurl for claim, conflict, primary, random default present (including the bootstrapped `dev` site), delete frees the name, `api` refused, a round-trip of a site's own list accepted, a boot warning on a duplicate.
2. **Resolve hosts by name; `LOCO_DOMAIN`; the `api` host; remove three-label hosts and `LOCO_DEFAULT_SITE`.** `host.rs`, `config.rs`, `server.rs`, the `loco/studio` seed claims `api`, the hosting suites rewritten, and every doc the change invalidates: `discovery.json`'s URL text, `docs/hosting.md`, `CLAUDE.md`, the READMEs. Note that an existing store must re-seed `loco/studio`. Acceptance: `benblog.localhost` serves its site; `api.localhost` serves Studio and its `/data` takes headers; a three-label host and the env var do nothing.
3. **Discovery URLs by primary domain.** Primary-domain URLs, `https` via `X-Forwarded-Proto`, port kept, host fallback. No cookie text. Acceptance: the discovery suite publishes and reaches the site by its primary domain.
4. **Session cookie and where login answers.** The per-host login table, the cookie (attributes, `Max-Age`, cleared on logout), the cookie read in `session_or_public` with bearer winning, `/auth/continue` with the parsed `return` check. Acceptance: login on a site host is 404; login on the apex returns a token and no cookie; login on `api` sets the cookie and returns the token; `continue` to an unknown origin, userinfo, or a backslash host is 400; the existing auth suites pass unchanged.
5. **Origin-checked cookie requests and CORS.** The extractor check against the project's computed origins, site-origin requests limited to their site, JSON-only cookie writes, CORS echo decided by `Origin` on preflight and request. Acceptance: Bob's cookie from Alice's app reads Alice's site and is 403 on Bob's project and on `/config`; a form-encoded cookie POST is refused; a credentialed preflight from a site origin is echoed; public requests keep `*`.
6. **Studio as the login.** Signup screen, the `?return=` hop (learning "logged in" from `/auth/me`), cookie session instead of a stored bearer token when served from `api`. Acceptance: in a browser on `LOCO_DOMAIN=localtest.me`, an app's sign-in link round-trips through Studio and the app's first credentialed `/data` call succeeds.
7. **`loco-client` cookie mode and the guide.** Cookie mode beside bearer mode; the guide's page signs in through the platform host and drops its login form. Acceptance: the discovery walk signs in on the platform host and the published page writes with the cookie.
