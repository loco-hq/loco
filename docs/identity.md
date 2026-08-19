# Identity and access

Target model. The code today is site-scoped users, username-only login, and a hardcoded metadata-editor site list. This document is the replacement. Implementation is the PR stack in `HANDOFF.md`.

Loco is Sanity-shaped: schema lives on the server; frontends are standalone and hold no secrets. A Loco account is someone who works *on* a project (Studio, CLI, Clippy). Visitors of a project's CloudFront app are not Loco users.

## Vocabulary

| Term | What it is |
|---|---|
| **Identity** | A person who logs in. Sessions and API keys belong to an identity. |
| **Account** | A handle that owns projects. `type` is `person` or `org`. The first segment of `{account}/{project}`. |
| **Membership** | Identity ↔ org, and identity ↔ project. |
| **Principal** | Who this request is: an identity, or the reserved `public` principal. |

Handles are unique across person and org accounts. There is no person `loco` and org `loco`.

A person account is 1:1 with an identity: signup creates both. An org account has no password and cannot log in. People act on `acme/crm` because of membership, not by becoming `acme`.

`public` is not an account and not a row in the identity table. Unauthenticated requests are this principal. What it may do is site/collection policy (PR 3), not a role grant.

There is no second user table for “users of the site.” Application users (customers of `pets.example.com`) are a later product and must not share these tables.

## Roles

**Org:** `owner` (developer on every `acme/*` project; manage members; delete the org) or `member` (nothing on a project until added to it).

**Project:** `developer` (schema + config + members + data) or `editor` (data only).

Creating a person-owned project grants the creator `developer` (they are the owner). Creating an org grants the creator org `owner`.

Personal accounts do not have members. If Alice wants a team, she creates an org and puts projects there.

## Request authz

1. Token → identity. No token → `public`.
2. `X-Project-Id` + `X-Site-Id` name the resource (version pin + dataset). They do not name the person.
3. Effective project access = org role on the account segment ∪ project role on `{account}/{project}`.
4. `/schema` and `/config` writes: identity with `developer` (or org `owner`) + draft version.
5. `/data` writes: `developer` or `editor`. `public` only where a site/collection flag allows it.
6. `/schema` reads: `developer` or `editor`, and (once PR 4 lands) `public` on a public-read site, pinned version only.

API keys are issued to an identity, then scoped to an account or one project, with `developer` or `editor`. Clippy/CI hold a key. Consumer bundles never do.

## What this replaces

| Today | After |
|---|---|
| Users/sessions/keys under `{user}/{project}/{site}/` | Identity is global. Login does not take a site. |
| `METADATA_EDITOR_SITES` (`loco/studio/studio`, `loco/cards/cards`) | Capability is on the member, not the site. |
| `require_can_edit_user` (session name == path segment) | Membership. `ben/pets` is a handle, not an ACL. |
| Studio logs into `loco/studio`, then overrides headers for data | One token. Headers only select the site. |
| Anonymous `/data` can create/update/delete as `public` | Policy flags. Default: no public write or delete. |

`loco/` is an org. People who edit `loco/studio` are members, not “username == loco”.

## Non-goals (this stack)

- Application users / site-customer auth
- Record-level security, field-level security, custom profiles
- Orgs logging in, impersonation, “switch to org”
- Nested orgs, teams, outside-collaborator vs member
- SSO, Clerk, a separate `auth.loco.dev` process
- Dual-running old site-scoped users (hard cut; no production data)
- Clippy, MCP, `@loco/client` (they consume this; they do not invent it)
