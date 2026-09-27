# Dependency inheritance via /schema/.../collection/list and field/{c}/list.
#
# Fixture dependency graph (every edge is a direct manifest dependency):
#   acme/sales@0-dev -> acme/crm@0-dev, base/core@0-dev
#   acme/crm@0-dev   -> base/core@0-dev
#
# acme/sales has: deal (with amount field)
# acme/crm has:   account (with company field)
# base/core has:  user (with email field), plus a tier field on account
#
# acme/sales depends on both others directly, so it sees all three
# collections. For acme/crm, only account + user. Field lists per collection
# resolve across self + direct deps. The direct-only rule — a transitive dep
# stays invisible — is direct_only.hurl.

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
jsonpath "$.data[*].name" includes "deal"
jsonpath "$.data[*].name" includes "account"
jsonpath "$.data[*].name" includes "user"

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

# 3. Field lists per collection resolve across self + direct deps for sales.
#    account gets company from acme/crm and tier from base/core.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/deal/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "amount"
jsonpath "$.data[0].type" == "float"

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/account/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.data[*].name" includes "company"
jsonpath "$.data[*].name" includes "tier"

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-dev/field/user/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "email"
jsonpath "$.data[0].type" == "string"

# 4. Unknown project returns 404
GET http://127.0.0.1:{{port}}/schema/acme/nonexistent/0-dev/collection/list
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.ok" == false
