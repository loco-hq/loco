# Identity and access

Target model. The implementing PR stack is merged; see the git history and the open issues for what remains. Public assignment now lives on the consuming version's manifest (`manifest.public_permission_sets`); `site` no longer carries the field.

Loco is Sanity-shaped: schema lives on the server; frontends are standalone and hold no secrets. A Loco account is someone who works *on* a project (Studio, CLI, Clippy). Visitors of a project's CloudFront app are not Loco users.

## Two kinds of people

| | **Loco user** | **Site user** (later) |
|---|---|---|
| Who | Someone who builds or edits a project | A visitor or customer of a deployed app (`pets.example.com`) |
| Lives | Global identity + person/org account | Per site. Not a Loco identity. |
| Auth | `POST /auth/login`, sessions, API keys | Not built yet. `public` is the unauthenticated stand-in. |
| ACL | Org/project **membership** (`owner` / `developer` / `editor`) | **Permission sets** assigned on the version the site pins |
| Examples | Alice in Studio, Clippy with a key | Anonymous reader, later a logged-in pet owner |

They must not share tables. A Loco login is never "log into this site as a customer." Site-user auth is delayed on purpose; the permission-set shape has to be right first, because `public` is already a site principal and packaged collections cannot carry that policy.

## Vocabulary

| Term | What it is |
|---|---|
| **Identity** | A Loco person who logs in. Sessions and API keys belong to an identity. |
| **Account** | A handle that owns projects. `type` is `person` or `org`. The first segment of `{account}/{project}`. |
| **Membership** | Identity ↔ org, and identity ↔ project. The Loco ACL. |
| **Principal** | Who this request is: a Loco identity, or the reserved `public` principal. |
| **Permission set** | A named, additive bundle of collection grants (`read`, `create`, `update`, `delete`). Stacked by union. |
| **Public assignment** | Which permission sets `public` gets: `public_permission_sets` on the **consuming version's manifest**. |

Handles are unique across person and org accounts. There is no person `loco` and org `loco`.

A person account is 1:1 with an identity: signup creates both. An org account has no password and cannot log in. People act on `acme/crm` because of membership, not by becoming `acme`.

`public` is not an account and not a row in the identity table. Unauthenticated `/data` requests are this principal. What it may do is the union of permission sets the **pinned version's manifest** assigns — not a project role, not a flag on a collection, and not a flag on the site. The site only names which version (and which dataset) this URL is.

## Why grants are not on the collection

A collection is schema. In a managed package, many projects install the same `contacts` collection. One deployment wants it world-readable; another does not. Baking `public_read` onto `contacts` would force every installer to inherit the package author's access policy, or to fork the collection.

Permission sets sit beside collections, in the same version:

```
${project}/versions/${version}/permission_sets/${name}
```

A set lists collections it grants, each with boolean verbs:

```yaml
# permission_sets/guestbook_read.yaml
label: Guestbook read
collections:
  - collection: guestbook
    read: true
```

Names are bare (`guestbook`) and resolve through `VersionSchema` (this version plus **direct** dependencies), so a consuming project can grant `contacts` even when that collection is owned by a package. Qualified `{project}.{name}` (e.g. `ben/crm.contacts`) is accepted and pins the owning project; a bare name is the common case and matches whichever collection that name resolves to (self first).

Sets are additive. There is no deny. Two sets that both mention `guestbook` stack: read from one and create from the other is read+create. Unspecified verbs default to false — listing a collection with only `read: true` is the read-only default.

The **consuming version** chooses which sets apply to `public`:

```yaml
# alice/agency versions/1.0.0/manifest.yaml
dependencies:
  - acme/crm@1.0.0
public_permission_sets:
  - public_contacts
```

That is the package story: two apps depend on the same `crm`; each version lists (or does not list) the sets it wants. Alice's agency is world-readable; Bob's internal tool depends on the same `acme/crm@1.0.0` and assigns nothing. A package may ship a recommended set (`public_contacts`); the consumer opts in by listing its name. Unknown names and grants for unknown collections are inert.

It is not two sites of one app with different config. Alice's `www` and `staging` both pin `alice/agency@1.0.0`, so they share policy and frontend; they differ by dataset. See [`hosting.md`](hosting.md).

Default: no sets assigned → `public` cannot list, get, insert, update, or delete. Public may do any verb an assigned set grants, including update and delete. Without record-level security that means anyone can mutate any row; opt in explicitly.

Loco `developer` / `editor` still have full `/data` access on the project. They do not go through permission sets. Sets are the data-plane for `public` (and, later, for site users) on the pinned version. Do not encode Studio capability as a permission set.

## Roles (Loco membership)

**Org:** `owner` (developer on every `acme/*` project; manage members; delete the org) or `member` (nothing on a project until added to it).

**Project:** `developer` (schema + config + members + data) or `editor` (data only).

Creating a person-owned project grants the creator `developer` (they are the owner). Creating an org grants the creator org `owner`.

Personal accounts do not have members. If Alice wants a team, she creates an org and puts projects there.

## Request authz

1. Token → Loco identity. No token → `public`.
2. `X-Project-Id` + `X-Site-Id` name the resource (version pin + dataset). They do not name the person.
3. Effective Loco project access = org role on the account segment ∪ project role on `{account}/{project}`.
4. `/schema` and `/config` writes: identity with `developer` (or org `owner`) + draft version.
5. `/data` for a Loco `developer` or `editor`: full CRUD.
6. `/data` for `public`: each verb (`read` / `create` / `update` / `delete`) if any set on the pinned version's manifest grants it on that collection. Authenticated Loco identities that are not members may use the public *read* hole (they are not `public` for writes).
7. `/schema` reads: `developer` or `editor` on any version of a project they belong to (no site headers). `public` (and authenticated non-members) on a site whose pinned version assigns at least one permission set, pinned version only — needs `X-Project-Id` + `X-Site-Id` (or a matching `Host`; see [`hosting.md`](hosting.md)), and path `{version}` must match the site pin. Any assigned set exposes the whole pinned version; per-collection schema visibility is later. Writes stay `developer` + draft.

API keys are issued to a Loco identity, then scoped to an account or one project, with `developer` or `editor`. Clippy/CI hold a key. Consumer bundles never do.

## What this replaces

| Today | After |
|---|---|
| Users/sessions/keys under `{user}/{project}/{site}/` | Identity is global. Login does not take a site. |
| `METADATA_EDITOR_SITES` (`loco/studio/studio`, `loco/cards/cards`) | Capability is on the member, not the site. |
| `require_can_edit_user` (session name == path segment) | Membership. `ben/pets` is a handle, not an ACL. |
| Studio logs into `loco/studio`, then overrides headers for data | One token. Headers only select the site. |
| Anonymous `/data` can create/update/delete as `public` | Version-assigned permission sets. Default: no public access. Verbs are whatever the set grants. |
| `public_read` / `public_create` on collection or site | `permission_set` metadata + `manifest.public_permission_sets` on the consuming version. |

`loco/` is an org. People who edit `loco/studio` are members, not “username == loco”.

## Non-goals (this stack)

- Application users / site-customer auth (the permission-set type is the hook; the user table is not)
- Record-level security, field-level security, custom profiles
- Expressing Loco `developer` / `editor` as permission sets
- Subtractive / deny rules
- Orgs logging in, impersonation, “switch to org”
- Nested orgs, teams, outside-collaborator vs member
- SSO, Clerk, a separate `auth.loco.dev` process
- Dual-running old site-scoped users (hard cut; no production data)
- Clippy, MCP, `@loco/client` (they consume this; they do not invent it)
