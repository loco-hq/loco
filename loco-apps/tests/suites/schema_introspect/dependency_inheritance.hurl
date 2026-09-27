# Dependency metadata via /schema, under the name-resolution rule: a bare
# name is the running project's, a dependency's is `{account}/{project}.{name}`
# — percent-encoded as one path segment (`acme%2Fcrm.account`).
#
# Fixture dependency graph (every edge is a direct manifest dependency):
#   acme/sales@0-dev -> acme/crm@0-dev, base/core@0-dev
#   acme/crm@0-dev   -> base/core@0-dev
#
# acme/sales has: deal (with amount field)
# acme/crm has:   account (with company field)
# base/core has:  user (with email field), plus a tier field declared on the
#                 name `account`, which base/core does not own
#
# Listings span self + direct deps and carry each item's `project`, so a
# client can build the qualified name. Lookups by name never fall through.
# The direct-only rule — a transitive dep stays invisible — is
# direct_only.hurl.

# Login as acme (global). Schema routes authorize by membership.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "acme", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. List collections for acme/sales@0-dev — should see all 3 (self + two direct deps)
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 3
jsonpath "$.data[?(@.name == 'deal')].project" nth 0 == "acme/sales"
jsonpath "$.data[?(@.name == 'account')].project" nth 0 == "acme/crm"
jsonpath "$.data[?(@.name == 'user')].project" nth 0 == "base/core"

# 2. List collections for acme/crm@0-dev — should see only 2 (no sales)
GET http://127.0.0.1:{{port}}/schema/acme/crm/0-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.data[*].name" includes "account"
jsonpath "$.data[*].name" includes "user"
jsonpath "$.data[*].name" not includes "deal"

# 3. A dependency's collection is reached qualified, and only qualified.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/collection/acme%2Fcrm.account
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "account"
jsonpath "$.data.project" == "acme/crm"

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/collection/account
Authorization: Bearer {{token}}
HTTP 404

# A qualified name for the running project itself is the bare name.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/collection/acme%2Fsales.deal
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "acme/sales"

# 4. Field lists. A bare collection is self's.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/deal/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "amount"
jsonpath "$.data[0].type" == "float"

# acme/crm.account has its owner's company. base/core's tier is declared on
# the name `account` by a project that is neither the owner nor acme/sales,
# so it is not a field of acme/crm's collection.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/acme%2Fcrm.account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "company"
jsonpath "$.data[0].project" == "acme/crm"

# Bare `account` is acme/sales' own, which it does not have.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/base%2Fcore.user/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "email"
jsonpath "$.data[0].type" == "string"

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/user/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 5. Installing base/core did not change acme/crm's own account either.
GET http://127.0.0.1:{{port}}/schema/acme/crm/0-dev/field/account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "company"

# 6. Unknown project returns 404
GET http://127.0.0.1:{{port}}/schema/acme/nonexistent/0-dev/collection/list
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.ok" == false
