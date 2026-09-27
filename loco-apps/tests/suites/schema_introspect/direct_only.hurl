# A version sees itself and its DIRECT dependencies — never a transitive one.
#
# Chain (each arrow is the only dependency its manifest declares):
#   acme/leads@0-dev -> acme/crm@0-dev -> base/core@0-dev
#
# base/core owns: collection user (field email), field tier on account,
#                 fieldset account/core_view, permission set core_read.
# acme/leads names core_read in public_permission_sets on purpose.
#
# Every assertion below on acme/leads would flip if direct_dependencies
# walked the manifest graph transitively. acme/crm, which depends on
# base/core directly, is the control: the same things are visible there.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "acme", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# ---------------------------------------------------------------------------
# /schema listings
# ---------------------------------------------------------------------------

# 1. Collections: self + acme/crm, not base/core's user.
GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[*].name" includes "lead"
jsonpath "$.data[*].name" includes "account"
jsonpath "$.data[*].name" not includes "user"

# 2. Fields on a collection leads can see: acme/crm's company, not the tier
#    base/core adds to account.
GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/field/account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "company"

# Control: acme/crm depends on base/core directly and does see tier.
GET http://127.0.0.1:{{port}}/schema/acme/crm/0-dev/field/account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[*].name" includes "tier"

# 3. Fields on base/core's own collection: none.
GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/field/user/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 4. Fieldsets and permission sets base/core owns are not listed.
GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/fieldset/account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/permission_set/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# ---------------------------------------------------------------------------
# Fall-through lookups by bare name (VersionSchema::collection / fieldset /
# permission_set). They walk self then deps; the deps must stop at direct.
# ---------------------------------------------------------------------------

# 5. Bare lookups that reach a direct dep still work — the negatives below
#    are not passing because the lookup is broken.
GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/collection/account
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/acme/crm/0-dev/collection/user
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/acme/crm/0-dev/fieldset/account/core_view
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/acme/crm/0-dev/permission_set/core_read
Authorization: Bearer {{token}}
HTTP 200

# 6. The same names from acme/leads are 404.
GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/collection/user
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/fieldset/account/core_view
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/acme/leads/0-dev/permission_set/core_read
Authorization: Bearer {{token}}
HTTP 404

# ---------------------------------------------------------------------------
# /data/query — strict qualified-name resolution (collection_in / field_in)
# ---------------------------------------------------------------------------

# 7. acme/crm.account resolves (direct dep); base/core.user does not, nor
#    does base/core's tier field on account.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
Content-Type: application/json
X-Project-Id: acme/leads
X-Site-Id: leads-dev
{
  "queries": {
    "account": {
      "collection": "acme/crm.account",
      "where": { "field": "acme/crm.company", "op": "eq", "value": "x" }
    },
    "user": { "collection": "base/core.user" },
    "tier": {
      "collection": "acme/crm.account",
      "where": { "field": "base/core.tier", "op": "eq", "value": "gold" }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.account.records" count == 0
jsonpath "$.data.account.error" not exists
jsonpath "$.data.user.diagnostics[0].kind" == "unknown_collection"
jsonpath "$.data.user.records" not exists
jsonpath "$.data.tier.diagnostics[0].kind" == "unknown_field"
jsonpath "$.data.tier.diagnostics[0].path" == "tier/where/field"

# Control: from acme/crm the same qualified names resolve.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
Content-Type: application/json
X-Project-Id: acme/crm
X-Site-Id: crm-dev
{
  "queries": {
    "user": { "collection": "base/core.user" },
    "tier": {
      "collection": "account",
      "where": { "field": "base/core.tier", "op": "eq", "value": "gold" }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.user.records" count == 0
jsonpath "$.data.tier.records" count == 0

# ---------------------------------------------------------------------------
# Public policy: acme/leads assigns core_read to public, but base/core owns
# it, so it does not resolve and public gets nothing.
# ---------------------------------------------------------------------------

# 8. Token-less read of acme/leads' own collection is refused.
POST http://127.0.0.1:{{port}}/data/query
Content-Type: application/json
X-Project-Id: acme/leads
X-Site-Id: leads-dev
{ "queries": { "lead": { "collection": "lead" } } }
HTTP 200
[Asserts]
jsonpath "$.data.lead.records" not exists
jsonpath "$.data.lead.diagnostics[0].kind" == "forbidden"
