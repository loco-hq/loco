# Hosting

Target model. Not implemented. Revises the site pin in [`identity.md`](identity.md).

A **version** is a snapshot of the app: schema, permission sets, and the frontend (HTML/CSS/JS, images the UI ships with, anything else that tree holds). YAML is one encoding of metadata, not the definition of it. A published version never changes. Rolling forward or back is pinning a site at a different version.

A **site** is a URL that points at a live version and a dataset. It is not where public policy lives, and it is not where the bundle lives.

Same static tree runs under Vite on a laptop (hosting is not involved) and from the version a site pins (hosting is just serving metadata). CDN adapters, when they exist, cache published versions. They do not change the snapshot.

## Vocabulary

| Term | What it is |
|---|---|
| **Version** | An immutable-once-published snapshot of a project: YAML instances *and* file trees under `versions/{version}/`. |
| **Draft** | A version whose name contains `-` (`0.0.1-dev`). Writable. Not infinitely cacheable. |
| **Published version** | A version with no `-`. Bytes never change. Cache forever. |
| **File-tree instance** | Metadata that is a directory of files, not a YAML document. First one: the frontend bundle. |
| **Bundle** | The file-tree instance at `${project}/versions/${version}/bundle`. A Vite `dist/` snapshot. |
| **Site** | A URL → `(version, dataset)`. Unversioned config, like today. The pointer is what moves. |
| **Host** | How that URL is named. v1: a subdomain of this Loco process. Later: a custom domain. |

```
site (URL, unversioned)
├── host      → subdomain (v1) / domain (later)
├── version   → which snapshot is live
└── dataset   → which lake partition

version (snapshot, versioned)
├── manifest              → deps + public_permission_sets
├── collections / fields / fieldsets / permission_sets
└── bundle/               → index.html, assets/, …
```

Two sites of the same app can pin the same version and different datasets (`www` → prod, `staging` → staging). They share policy and frontend: those are the snapshot. Two apps that both depend on `crm` pick their own policies on *their* versions — that is not two configs of one site.

## Why the bundle is versioned metadata

The product move is one select box: live version `0.0.1` → `0.0.2`. That has to move schema, public policy, *and* the UI together. A CSS fix that is not a schema change is `0.0.3` with the same collections. A rollback is pinning `0.0.1` again. The previous tree is still there; nothing in the adapter "undoes."

Replace-in-place on the site (the earlier draft of this doc) made rollback the storage layer's problem and let the HTML drift from the schema the site claims to pin. That is the same class of lie as `/data` seeing a different world than `/schema`.

Published versions are already write-locked (`is_draft_version`). Extending that lock to the file tree is how you get the cache property: for a published version, every byte is immutable, YAML or not. Drafts stay mutable, including `vite build` into the draft bundle.

This is not a `bundle` YAML file that *points at* files. The files are the instance.

## Why sites are URLs, not policy

`public_permission_sets` on the site was the wrong home. A site's job is to match a request to an app: which snapshot, which data, at which URL. Policy is part of what you publish.

The package story is two *apps* depending on the same `crm`, not two sites of one app with different config. `acme/crm` ships `contacts` and a recommended set. Alice's agency wants it world-readable; Bob's internal tool does not. Each consuming **version** opts in (or doesn't) on its own manifest:

```yaml
# alice/agency versions/1.0.0/manifest.yaml
dependencies:
  - acme/crm@1.0.0
public_permission_sets:
  - public_contacts

# bob/ops versions/1.0.0/manifest.yaml
dependencies:
  - acme/crm@1.0.0
public_permission_sets: []
```

Same `crm@1.0.0`. Different public policies. Grants still are not on `contacts` — baking `public_read` onto the package would force every installer to inherit it. A package may ship `public_contacts`; the consumer lists the name. Unknown names and grants for unknown collections stay inert.

What goes away is two sites of *alice/agency@1.0.0* with different public policies. `www` and `staging` share the snapshot; they differ by dataset. Different policy or different frontend is a different version (or a different project).

Dataset stays on the site. Staging vs prod data is a URL split, not a snapshot split.

[`identity.md`](identity.md) follows this: assignment lives on the consuming version's manifest, not on the site.

## Metadata that is not YAML

Today every type is a YAML document and `YamlFsAdapter` keys `path + ".yaml"`. That is too narrow. A type definition can be a **document** (YAML, current) or a **file tree** (a directory at the pathTemplate, no `.yaml`).

```yaml
# schemas/types/bundle.yaml
description: "Static file tree shipped with a version"
kind: files
pathTemplate: "${project}/versions/${version}/bundle"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  version:
    type: slug
    createOnly: true
```

No body properties. Codegen still emits `to_path` / `from_path` and a store keyed by that path. The store holds a directory, not a struct of fields. Later file-tree types (seed fixtures, a shipped icon set) are the same kind, different pathTemplate.

What this is not:

- **Not the file lake.** Article images, user uploads, per-record attachments are data. They live in a dataset and follow record authz. A logo the UI ships is bundle metadata. Mixing those is how you accidentally version customer photos.
- **Not codegen of HTML.** File-tree instances are opaque bytes. No `from_yaml`, no accessors per asset.
- **Not a parallel HostAdapter.** Storage is the metadata persistence layer (local filesystem first, the way YAML already works). A CDN is a cache of *published* version trees, not a second place a site "deploys to."

On disk, next to today's YAML:

```
schemas/instances/ben/blog/
  project.yaml
  datasets/prod.yaml
  sites/www.yaml
  versions/0.0.1/
    manifest.yaml
    collections/article.yaml
    fields/article/…
    permission_sets/article_read.yaml
    bundle/
      index.html
      assets/index-xxxxx.js
```

`YamlFsAdapter` becomes the document half of a persistence adapter that can also put/get files under a pathTemplate prefix. Walk, write, delete-by-prefix, copy-version all see both halves. Copy-version is the publish primitive: it duplicates YAML *and* the bundle tree into a new id (optionally without `-`). Without that, "select box to `0.0.2`" has nothing to point at.

Writes to a file-tree instance are whole-tree replace (a zip of Vite `dist/`, `index.html` at the zip root), draft-only, same developer bar as `/schema`. No patch of one hashed asset; the snapshot is the unit.

## Immutability and cache

| | Draft (`0.0.1-dev`) | Published (`0.0.1`) |
|---|---|---|
| YAML + bundle writes | Yes, developer | No |
| Cache | Short / none | Bytes never change |
| Site pin | Dev URL can point here | Prod URL points here |

"Cache forever" applies to **version bytes**, not to the site pointer.

- `GET` of a file from a published version: `Cache-Control: immutable` (or equivalent). CDN keyed by `{project}/{version}/…` never invalidates except on delete.
- The site's `/` (the HTML the visitor actually loads) is a pointer. Pin moves `0.0.1` → `0.0.2` must be visible. `index.html` at the site URL is short-cache or revalidate. Hashed Vite assets are already cache-safe by filename; serving them from the version tree with immutable headers is enough. Do not cache the pin.

Draft versions are not this deal. A live `*-dev` site is an editor preview.

## HTTP

Reserved prefixes always win, on every host:

`/data` `/schema` `/config` `/auth`

There is no `/host` prefix. The bundle is schema. Deploy is a draft write:

| Method | Path | Who | What |
|---|---|---|---|
| PUT | `/schema/{account}/{project}/{version}/bundle` | developer, draft only | Replace the file tree (zip) |
| GET | `/schema/{account}/{project}/{version}/bundle` | same as other `/schema` reads | `{ hash, uploaded_at, size }` (not the files) |
| DELETE | `/schema/{account}/{project}/{version}/bundle` | developer, draft only | Drop the tree |

Serving the files is not a `/schema` GET. It is the site URL (below).

### Sites as hosts

v1: each site is a subdomain of this process. Derived, unique, no extra uniqueness table:

```
{site}.{project}.{account}.<listen-host>
```

`www` on `ben/blog` at `:3000` → `www.blog.ben.localhost:3000`. Custom domains later replace the derived name; they do not change the pin.

`sites/www.yaml`:

```yaml
label: Public blog
version: 0.0.1
dataset: prod
```

No `public_permission_sets`. No `host` field until custom domains need an override. The path *is* `{project}/sites/{name}`; the DNS name is computed from it.

Request routing:

1. If `Host` matches `{site}.{project}.{account}.<listen-host>`, that site is the request's site. `/data` and `/schema` may omit `X-Project-Id` / `X-Site-Id`; the host filled them in. Headers, if sent, must agree.
2. If `Host` is the apex (`localhost:3000`, the listen address), there is no site. API only, unless `LOCO_DEFAULT_SITE` names one — then the apex serves that site's bundle at `/` the same way a subdomain would. This is the "one process is the blog" case, and the generic form of issue #30.
3. Fallback after API nests: files from the **pinned version's bundle**. SPA fallback to that tree's `index.html` for extensionless / `Accept: text/html` misses. Missing hashed assets 404. Missing bundle 404s `/`, does not fail boot.

Apex without a default site is today's API-only process. Vite-dev talks to the apex with headers (or its proxy). Hosting is not involved.

Local Vite, `vite preview`, and an agent's browser tool still run the same `dist/` against the apex. They do not need a subdomain. Subdomains are how *hosted* visitors hit a site.

## Authz

1. Token → Loco identity. No token → `public`.
2. Bundle PUT/DELETE: project `developer` (or org `owner`) + draft version. Same as any `/schema` write.
3. Bundle metadata GET: same as other `/schema` reads (developer/editor, or `public` on a site whose pinned version assigns at least one permission set).
4. Served bundle files at the site URL: public. No token. The snapshot's face holds no secrets.
5. `/data` for `public`: union of permission sets the **pinned version's manifest** assigns. Unspecified verbs false. Loco `developer` / `editor` still bypass sets.

Consumer bundles never contain API keys. That is `identity.md`. Versioning the bundle does not relax it.

## The loop

"Yo, I want a blog":

1. Create project `ben/blog` (bootstraps `0.0.1-dev`, dataset `dev`, site `dev`).
2. Collection `article`. Permission set `article_read`. Manifest lists `public_permission_sets: [article_read]`.
3. Write a Vite app that calls `/data/article/list`. Locally: headers `X-Project-Id` / `X-Site-Id`, Vite proxy or CORS to the apex. No token. **Hosting is not involved.**
4. `vite build`. `PUT /schema/ben/blog/0.0.1-dev/bundle` with the zip (or write the tree on disk under the draft version). Site `dev` already pins `0.0.1-dev`.
5. `http://dev.blog.ben.localhost:3000/` (or apex with `LOCO_DEFAULT_SITE`) is the blog against draft metadata.
6. Publish: copy-version to `0.0.1` (YAML + bundle, now immutable). Point `www` at `0.0.1` + `prod`. That is the select box.
7. Next release: iterate on a new draft, publish `0.0.2`, flip the pin. Flip it back to roll back.

CLI and MCP wrap 1–2, 4, and 6. They do not invent a second hosting model.

## Issue 30, generically

[#30](https://github.com/loco-hq/loco/issues/30) wants `cargo run` to be API + Studio: `ServeDir` over `loco-studio/dist`, SPA fallback, JSON 404s under API prefixes, missing `dist/` logs and starts, Vite-dev unchanged.

That is serving *a pinned version's bundle* at a URL, with reserved prefixes winning. Hardcoding `loco-studio/dist` in `server.rs` cannot host a blog and cannot roll back.

| #30 | This model |
|---|---|
| `ServeDir` after API nests | Fallback after reserved prefixes, files from the site's pinned version |
| Unmatched non-API path → `index.html` | SPA fallback on that version's bundle |
| Mistyped `/data/...` stays JSON 404 | Reserved prefixes always win |
| Missing `dist/` logs and skip | Missing bundle: boot succeeds, `/` 404s |
| `cargo run` + built `dist/` is API + Studio at `:3000` | Apex + `LOCO_DEFAULT_SITE=loco/studio/studio`, that site pinning a version that has a bundle |
| Vite-dev on `:5174` unchanged | Local column. Hosting is not involved |

Do not read `loco-studio/dist` from `server.rs`. Do not default `LOCO_DEFAULT_SITE` inside the binary. The README can show Studio as the apex default for people working on Loco; a blog process sets it to `ben/blog/www`. Close #30 when a site URL serves a version's file tree with those rules — not when a `ServeDir` lands.

## Studio

Do not delete it in this stack. Do not special-case it either.

Studio is a Vite app. Its `dist/` is the bundle of some `loco/studio` version. `cargo run` does not bake it in. Inner loop remains `npm run dev -w loco-studio`. Same-origin needs `API_ORIGIN = ''` when the bundle is served from a site URL; Vite-dev keeps the proxy.

Deleting Studio before CLI and MCP exist leaves humans with YAML and curl. Hosting makes that call cheap later: Studio is already a versioned bundle.

## Frontend contract

A hosted (or local) app:

- Holds no secrets.
- Locally, knows `projectId` and `siteId` (headers). On a site host, the server infers them; the app can still send them if they match.
- Calls origin-absolute API paths (`/data/...`, `/auth/...`).
- Builds with relative Vite `base` (`base: './'` on build — Studio already does).
- Works with no token when the pinned version's public policy is enough.

## What this replaces

| Today | After |
|---|---|
| Metadata = YAML documents | Metadata = a version directory: YAML documents + file trees |
| Site pins version + dataset + `public_permission_sets` | Site pins version + dataset + URL. Policy is on the manifest |
| Bundle nowhere (python http.server / Vite) | Bundle at `${project}/versions/${version}/bundle` |
| Rollback = hope you have files | Rollback = pin the previous version |
| Publish = hyphen convention, empty new version | Copy-version duplicates YAML *and* file trees |
| [#30](https://github.com/loco-hq/loco/issues/30) `ServeDir` of Studio `dist/` | Any site URL serves its pinned version's bundle |
| `examples/public-page` as a second process | Bundle on `loco/demo@…` served at `www.demo.loco.…` |
| Cache nothing, or cache ad hoc | Published version bytes are immutable |

## Non-goals (this stack)

- CDN / S3 adapters (the immutability rule is what makes them a cache, not a second source of truth)
- Custom domains, TLS
- Injecting config into `index.html` at serve time
- Storing frontend *source* (that is git). The bundle is the build.
- SSR, Next, Node at runtime
- File lake, per-record attachments, user-uploaded images
- Site-user auth (see `identity.md`)
- CLI, MCP, `@loco/client` (they consume this)
- Deleting Studio, baking its `dist/` into the binary, hardcoding `loco-studio/dist` in `server.rs`
- Multiple named bundles per version (`www` vs `admin` as two trees). Different face, different version or different project.
- Cookies. Bearer stays. CORS `*` stays legal for the local/Vite case.
