# Dependency inheritance via /schema/.../collection/list and field/{c}/list.
#
# Fixture dependency graph:
#   acme/sales@0-draft -> acme/crm@0-draft -> base/core@0-draft
#
# acme/sales has: deal (with amount field)
# acme/crm has:   account (with company field)
# base/core has:  user (with email field)
#
# When listing collections for acme/sales, we should see all three (deal +
# account + user). For acme/crm, only account + user. Field lists per
# collection should resolve across the dependency closure.

# Login as acme (global). Schema reads/writes still send loco/cards headers.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "acme", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. List collections for acme/sales@0-draft — should see all 3 (self + transitive deps)
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 3
jsonpath "$.data[*].name" includes "deal"
jsonpath "$.data[*].name" includes "account"
jsonpath "$.data[*].name" includes "user"

# 2. List collections for acme/crm@0-draft — should see only 2 (no sales)
GET http://127.0.0.1:{{port}}/schema/acme/crm/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.data[*].name" includes "account"
jsonpath "$.data[*].name" includes "user"
jsonpath "$.data[*].name" not includes "deal"

# 3. Field lists per collection resolve across the dep closure for sales.
#    Each collection in the closure has exactly one field declared.
GET http://127.0.0.1:{{port}}/schema/acme/sales/0-draft/field/deal/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "amount"
jsonpath "$.data[0].type" == "float"

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-draft/field/account/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "company"
jsonpath "$.data[0].type" == "string"

GET http://127.0.0.1:{{port}}/schema/acme/sales/0-draft/field/user/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "email"
jsonpath "$.data[0].type" == "string"

# 4. Unknown project returns 404
GET http://127.0.0.1:{{port}}/schema/acme/nonexistent/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404
[Asserts]
jsonpath "$.ok" == false
