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
- **Charset:** a host label — `[a-z0-9-]`, 1–63, not starting or ending with `-` — or `@`. Checked in `http/names.rs`.
- **Unique across the server.** At boot, `SchemaStore` loading builds an index name → `(project, site)`. A site create or update that names a domain another site already has is 409 naming that site's project and site; the check and the write run under `PINS` (`http/project_config.rs`). Two sites on disk claiming one name is a boot warning naming both; neither gets the name until one is removed.
- **Exactly one primary.** On write, more than one `primary: true` is 400. None marked means the first entry is primary.
- **Random default.** Site create assigns one random name (three words from a 50-word list compiled into the binary, `unicorn-particle-horse` — 125,000 names, enough for now and easy to grow — retried on collision) and stores it in `domains`. Random names are not secrets. It is an ordinary entry: it does not change on restart, and it can be removed once the site has another name.
- **Delete releases.** Deleting a site — or its project — releases every name on it. To keep a name, move it to another site first (remove it here, add it there, two writes under `PINS`).
- **Who may claim:** whoever may write the site (developer or org owner of its project). There is no per-account limit yet; a limit, or claimable names as a paid feature with random names on the free plan, comes later.

### Reserved names are seed records

The server owner claims names by committing them on seed sites, the same way seed projects are committed (`schemas/seed/`, copied into the store at boot when absent, never overwritten — `loco-apps/src/seed.rs`). They are claimed before any user exists.

`loco/studio/sites/studio.yaml` carries `domains: [{name: api}]`. That is how Studio becomes the platform host's pages, and the only thing that makes it so: Studio is an ordinary bundle on an ordinary site. Replacing Studio is changing which seed site claims `api`.

`@` is unclaimed and serves nothing for now.

### The one special name

`api` is the only name the server treats differently, and the difference is about the host, not the site that claims it:

- Its API is **never pinned**: `/data` and `/actions` take their site from `X-Project-Id` / `X-Site-Id`, as today's apex does. This is the API for anything that works across projects — Studio, the CLI, MCP.
- It is the **full-access origin** for a session (see "Access from a hosted app").
- Non-API paths serve the claiming site's pinned bundle, as any site host does.
- Only a seed may claim it: a `/config` write that names `api` is 400.

### Custom domains, later

A custom domain is an entry whose `name` contains a dot (`inventory.example.com`) and that also carries verification state (a DNS TXT record the owner sets) and certificate status. The table is already keyed by full name for that reason. This doc does not specify them.

## Resolving a request

`http/host.rs` `resolve_site` becomes one lookup:

1. Take `Host`, drop the port, lowercase it, trim a trailing dot.
2. Equal to the platform domain → name `@`. One label followed by `.{platform domain}` → that label. Anything else → no site.
3. `api`: the bundle is the claiming site's; the API is not pinned (step 5).
4. Any other name found in the index is that site: absent `X-Project-Id` / `X-Site-Id` are filled in, and a header naming another site is 400 — as a subdomain is today, with no exception.
5. Otherwise the API answers with `/data` and `/actions` taking their site from the headers, and `/` is the JSON 404 with `see: /.well-known/loco.json`.

`site_ref_from_host` (the three-label parse), `parse_site_ref`, `Config::default_site`, and `AppState::default_site` are deleted, with `LOCO_DEFAULT_SITE`. A three-label link stops resolving.

`LOCO_DOMAIN` is a `Config` field (`loco-apps/src/config.rs`), default `localhost`. Boot logs the platform domain, the platform host, and one line per seed-claimed name.

## Access from a hosted app

identity.md's model stands: only Loco users log in, access is membership, and headers select a site. What changes is **where** the login is typed and **which pages may use it**. This is Sanity's model: one universal login; each project lists the origins allowed to act with it ([Sanity: CORS and browser security](https://www.sanity.io/docs/content-lake/browser-security-and-cors)).

### One session, on the platform host

- **Password login and signup move to the platform host.** `POST /auth/login` and `POST /auth/users` answer only on `api.{platform domain}`; elsewhere they are 404 with `see`. A hosted app never shows a password field.
- **Login sets an HttpOnly session cookie** on the platform host: `HttpOnly`, `Secure` (except on `localhost`), `SameSite=Lax`, host-only (no `Domain` attribute), `Path=/`. JavaScript cannot read it, so a page's code cannot copy it and use it elsewhere.
- **The login and signup pages are Studio's.** Studio is the platform host's bundle, so its login screen posts to its own origin and the cookie lands on the platform host. Logging into Studio is the global login. Studio gains a signup screen.
- **Returning to an app.** An app sends the user to `https://api.{platform domain}/?return={url}`. Studio logs them in, or, if they already are, continues at once with no confirmation step, by navigating to `GET /auth/continue?return={url}`. The server answers 302 to `return` only when it is a site origin of a site that exists, or the platform host; anything else is 400. The check is on the server, so the login page cannot be used to bounce a user to an arbitrary site, whatever bundle claims `api`.
- **Bearer tokens stay for tools.** The CLI, MCP, CI, and Studio-in-Vite keep `Authorization: Bearer` with a session token or an API key, as today. A bearer request is not subject to the origin check below; a browser page never holds one.

### Each project lists its origins

A **credentialed browser request** carries the session cookie and an `Origin` header. Its allowed origins are computed, not configured:

- the platform host (`https://api.{platform domain}`), which may act on any project the user can access, as a bearer token does today;
- every site origin of the project the request targets. A request from one of those origins acts **on that site only**: its `/data` and `/actions`, plus `/auth/me` and `POST /auth/logout`. The site is the one the origin's name resolves to; a header naming another site is 400. `/config` and `/schema` from a site origin are 403.

Anything else is 403 `origin {origin} may not use this session for {project}`. The check is on the server, in the request extractors (`http/scope/`), against the request's `Origin` — it does not rely on the browser.

So Bob, an editor on `alice/inventory`, opens `aliceapp.lococo.run`. The page calls `https://api.lococo.run/data/...` with `credentials: 'include'`. The browser sends Bob's cookie and `Origin: https://aliceapp.lococo.run`. The server resolves that origin to `alice/inventory`'s site, checks Bob's membership, and serves that site's data. The same page calling for one of Bob's own projects is 403: its origin is not on that project's list.

### CORS

`cors_layer` in `server.rs` stops being `Any` for credentialed requests:

- A request with no cookie and no `Authorization` (public) keeps `Access-Control-Allow-Origin: *`, no credentials.
- A request from an allowed origin gets that origin echoed in `Access-Control-Allow-Origin`, `Access-Control-Allow-Credentials: true`, and `Vary: Origin`.
- Bearer requests keep today's behavior.

### Forged requests

Every `*.lococo.run` page is the same site to the browser, so `SameSite=Lax` lets another app's page make a same-site request that carries the cookie. Two server rules cover it:

- A cookie-authenticated request must carry an `Origin` the project allows (above). A same-site page that is not on the list is 403.
- A cookie-authenticated write must be `Content-Type: application/json`. A plain HTML form cannot send that, and a script that does is preflighted, so it is held to the origin list.

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
- Studio-in-Vite and local example apps keep bearer tokens through their proxies and are not affected.

## The API on every host

Unchanged: every host answers the whole API, so a hosted page's token-less calls work on its own origin. On a site's own host, `/data` and `/actions` are pinned to that site. A **logged-in** page calls the platform host instead, because that is where the cookie is sent.

## What the discovery document says

`src/discovery.json` and `discovery.rs` stop describing three-label hosts and `LOCO_DEFAULT_SITE`:

- A site's URL is its primary domain. Steps that create a site read `data.domains` from the response.
- Signup and login point at `https://api.{platform domain}`. The guide's page links to `/login?return=…` and calls the platform host with `credentials: 'include'` (replacing the #163 page's own login form).
- URLs use `https` when the request arrived over TLS (`X-Forwarded-Proto: https` from the proxy, #107), and fall back to the literal `{platform domain}` when `Host` is not a valid hostname.

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

None.

## Implementation issues

Each is one PR; the orchestrator files them, replacing #145.

1. **Domains on sites.** `site_domain` list on `site.yaml`, name check, the boot index, 409 on a taken name under `PINS`, one-primary rule, random default on create (50-word list), release on delete, `api` claimable only by a seed. Acceptance: Hurl for claim, conflict, primary, random default present, delete frees the name, `api` refused on `/config`, boot warning on a duplicate.
2. **Resolve hosts through the index; `LOCO_DOMAIN`; the `api` host; remove three-label hosts and `LOCO_DEFAULT_SITE`.** `host.rs`, `config.rs`, `server.rs`, the `loco/studio` seed claims `api`, hosting suites rewritten. Acceptance: `benblog.localhost` serves its site; `api.localhost` serves Studio and its `/data` takes headers; a three-label host and the env var no longer do anything.
3. **Discovery document on domains.** Primary-domain URLs, `https` via `X-Forwarded-Proto`, host fallback. Acceptance: discovery suite publishes and reaches the site by its primary domain.
4. **Session cookie and login on the platform host.** Login/signup answer only on `api`, HttpOnly cookie, `/auth/continue` with the `return` check. Acceptance: login on a site host is 404; login on `api` sets the cookie; `continue` to an unknown origin is 400.
5. **Origin-checked credentialed requests and CORS.** Extractor check against the project's computed origins, site-origin requests limited to their site, JSON-only cookie writes, CORS echo with credentials. Acceptance: Bob's cookie from Alice's app reads Alice's site and is 403 on Bob's project and on `/config`; a form POST is refused; public requests keep `*`.
6. **Studio as the login.** Signup screen, the `?return=` hop, cookie session instead of a stored bearer token. Acceptance: in a browser on `LOCO_DOMAIN=localtest.me`, an app's sign-in link round-trips through Studio and the app's first `/data` call succeeds.
7. **`loco-client` cookie mode and the guide.** Acceptance: the discovery walk logs in on the platform host and the published page writes with the cookie.
