# Name resolution (#28): an unqualified name always means the project that
# owns the running version; a dependency's collection, field, fieldset, or
# permission set is reachable only as `{account}/{project}.{name}`. In a path
# segment the qualified name is percent-encoded: `acme%2Fcrm.contacts`.
#
# Fixtures:
#   alice/app@1-dev  — site `dev`. Owns `contacts` (name) and `notes` (body),
#                      and the set `open` (public read on bare `contacts`).
#                      Depends directly on acme/crm@1.0 and beta/crm@1.0.
#                      Assigns [open, dep_public] to public.
#   acme/crm@1.0     — owns `contacts` (email, fieldset `summary`) and
#                      `deals` (amount); sets `open` (read `deals`) and
#                      `dep_public` (read + create bare `notes`). Depends on
#                      base/lib@1.0.
#   beta/crm@1.0     — owns `contacts` (phone).
#   base/lib@1.0     — owns `things`. Transitive from alice/app.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# ---------------------------------------------------------------------------
# 1. A bare name never reaches a dependency — any of the four kinds.
# ---------------------------------------------------------------------------

# Collection: `deals` exists only in acme/crm.
GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/collection/deals
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/data/deals/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 404

# Bare `contacts` is alice/app's, whatever the deps own.
GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/collection/contacts
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "alice/app"

# Field: alice/app's contacts have `name` only. acme/crm's `email` and
# beta/crm's `phone` are fields of *their* contacts.
GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/field/contacts/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "name"

GET http://127.0.0.1:{{port}}/data/contacts/fields
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "name"

POST http://127.0.0.1:{{port}}/data/contacts/add
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"email": "a@example.com"}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "unknown_field"
jsonpath "$.diagnostics[0].path" == "email"

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/field/deals/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# Fieldset: `summary` is acme/crm's, on acme/crm's contacts.
GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/fieldset/contacts/summary
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/fieldset/contacts/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# A bare set name on a dependency's collection is still this project's set.
GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/fieldset/acme%2Fcrm.contacts/summary
Authorization: Bearer {{token}}
HTTP 404

# Permission set: both alice/app and acme/crm have `open`. Bare is alice's.
GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/permission_set/open
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "alice/app"

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/permission_set/dep_public
Authorization: Bearer {{token}}
HTTP 404

# /data/query: a bare field on a dependency's collection is unknown.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "deals": { "collection": "deals" },
    "email": {
      "collection": "acme/crm.contacts",
      "where": { "field": "email", "op": "eq", "value": "x" }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.deals.diagnostics[0].kind" == "unknown_collection"
jsonpath "$.data.email.diagnostics[0].kind" == "unknown_field"

# ---------------------------------------------------------------------------
# 2. A qualified name reaches a direct dependency.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/collection/acme%2Fcrm.deals
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "acme/crm"

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/field/acme%2Fcrm.deals/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "amount"

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/fieldset/acme%2Fcrm.contacts/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "summary"

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/fieldset/acme%2Fcrm.contacts/acme%2Fcrm.summary
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "acme/crm"

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/permission_set/acme%2Fcrm.open
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "acme/crm"
jsonpath "$.data.collections[0].collection" == "deals"

POST http://127.0.0.1:{{port}}/data/acme%2Fcrm.deals/add
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"amount": 12.5}
HTTP 201
[Captures]
deal_id: jsonpath "$.data.id"

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.deals/get/{{deal_id}}
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.fields.amount" == 12.5

PUT http://127.0.0.1:{{port}}/data/acme%2Fcrm.deals/update/{{deal_id}}
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"amount": 20}
HTTP 200

# The lake key is {owner_project}.{name}: /data/query reads the same record.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{ "queries": { "deals": { "collection": "acme/crm.deals" } } }
HTTP 200
[Asserts]
jsonpath "$.data.deals.records" count == 1
jsonpath "$.data.deals.records[0].id" == "{{deal_id}}"

DELETE http://127.0.0.1:{{port}}/data/acme%2Fcrm.deals/delete/{{deal_id}}
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200

# A qualified name for the running project is the bare name.
GET http://127.0.0.1:{{port}}/data/alice%2Fapp.contacts/fields
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data[0].name" == "name"

# ---------------------------------------------------------------------------
# 3. A qualified name for a transitive or unknown project is a 404.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/collection/base%2Flib.things
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/data/base%2Flib.things/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/field/base%2Flib.things/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/collection/nobody%2Fhere.contacts
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/data/nobody%2Fhere.contacts/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/app/1-dev/permission_set/base%2Flib.open
Authorization: Bearer {{token}}
HTTP 404

# A qualified collection in a dependency that lacks it.
GET http://127.0.0.1:{{port}}/data/beta%2Fcrm.deals/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 404

# ---------------------------------------------------------------------------
# 4. Two dependencies that share a collection name are both addressable,
#    each with its own fields and its own records.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/fields
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "email"

GET http://127.0.0.1:{{port}}/data/beta%2Fcrm.contacts/fields
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "phone"

POST http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/add
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"email": "acme@example.com"}
HTTP 201

POST http://127.0.0.1:{{port}}/data/beta%2Fcrm.contacts/add
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"phone": "555-0100"}
HTTP 201

# Each validates against its own owner's fields.
POST http://127.0.0.1:{{port}}/data/beta%2Fcrm.contacts/add
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"email": "acme@example.com"}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "unknown_field"

POST http://127.0.0.1:{{port}}/data/contacts/add
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"name": "Own"}
HTTP 201

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].fields.email" == "acme@example.com"

GET http://127.0.0.1:{{port}}/data/beta%2Fcrm.contacts/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].fields.phone" == "555-0100"

GET http://127.0.0.1:{{port}}/data/contacts/list
Authorization: Bearer {{token}}
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].fields.name" == "Own"

# ---------------------------------------------------------------------------
# 5. Public policy. alice/app assigns its own `open` (read on bare
#    `contacts`) and a bare `dep_public` it never defined.
# ---------------------------------------------------------------------------

# alice/app's own contacts: open's bare grant applies.
GET http://127.0.0.1:{{port}}/data/contacts/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# The same bare grant does not open a dependency's same-named collection —
# not on the /data routes, and not through /data/query.
GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 403

GET http://127.0.0.1:{{port}}/data/beta%2Fcrm.contacts/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 403

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "own": { "collection": "contacts" },
    "acme": { "collection": "acme/crm.contacts" }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.own.records" count == 1
jsonpath "$.data.acme.diagnostics[0].kind" == "forbidden"

# Bare `open` is alice's set, so acme/crm's `open` (read on its deals) is not
# assigned.
GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.deals/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 403

# The leak this issue recorded: a set the consumer never wrote, named bare,
# must not resolve to a dependency's set of that name and open the
# consumer's own collection. `dep_public` is inert.
GET http://127.0.0.1:{{port}}/data/notes/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 403

POST http://127.0.0.1:{{port}}/data/notes/add
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"body": "hi"}
HTTP 403

# Opting in qualified reaches acme/crm's set — whose bare `notes` grant
# means acme/crm's notes, never alice/app's.
PUT http://127.0.0.1:{{port}}/schema/alice/app/1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"public_permission_sets": ["open", "acme/crm.dep_public", "acme/crm.open"]}
HTTP 200

GET http://127.0.0.1:{{port}}/data/notes/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 403

POST http://127.0.0.1:{{port}}/data/notes/add
X-Project-Id: alice/app
X-Site-Id: dev
Content-Type: application/json
{"body": "hi"}
HTTP 403

# acme/crm.open does apply now: its bare `deals` is acme/crm's deals.
GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.deals/list
X-Project-Id: alice/app
X-Site-Id: dev
HTTP 200
