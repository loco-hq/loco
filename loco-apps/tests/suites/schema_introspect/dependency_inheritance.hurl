# Dependency inheritance via /schema/collections introspection
#
# Fixture dependency graph:
#   acme/sales -> acme/crm -> base/core
#
# acme/sales has: deal (with amount field)
# acme/crm has:   account (with company field)
# base/core has:  user (with email field)
#
# When introspecting acme/sales, we should see all three namespaces.
# When introspecting acme/crm, we should see crm + core but NOT sales.

# 1. Introspect sales-dev — should see all 3 namespaces
GET http://127.0.0.1:{{port}}/schema/collections
X-Site-Id: sales-dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 3
# First namespace is the root: acme/sales
jsonpath "$.data[0].namespace" == "acme/sales"
jsonpath "$.data[0].collections" count == 1
jsonpath "$.data[0].collections[0].name" == "deal"
jsonpath "$.data[0].collections[0].collection_fields" count == 1
# Second namespace is the direct dep: acme/crm
jsonpath "$.data[1].namespace" == "acme/crm"
jsonpath "$.data[1].collections" count == 1
jsonpath "$.data[1].collections[0].name" == "account"
jsonpath "$.data[1].collections[0].collection_fields" count == 1
# Third namespace is the transitive dep: base/core
jsonpath "$.data[2].namespace" == "base/core"
jsonpath "$.data[2].collections" count == 1
jsonpath "$.data[2].collections[0].name" == "user"
jsonpath "$.data[2].collections[0].collection_fields" count == 1

# 2. Introspect crm-dev — should see only 2 namespaces (no sales)
GET http://127.0.0.1:{{port}}/schema/collections
X-Site-Id: crm-dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.data[0].namespace" == "acme/crm"
jsonpath "$.data[0].collections[0].name" == "account"
jsonpath "$.data[1].namespace" == "base/core"
jsonpath "$.data[1].collections[0].name" == "user"

# 3. Verify field details are included in inheritance
#    The deal collection's amount field should have correct metadata
GET http://127.0.0.1:{{port}}/schema/collections
X-Site-Id: sales-dev
HTTP 200
[Asserts]
# deal's amount field
jsonpath "$.data[0].collections[0].collection_fields[0][1].name" == "amount"
jsonpath "$.data[0].collections[0].collection_fields[0][1].type" == "float"
# account's company field (inherited from crm)
jsonpath "$.data[1].collections[0].collection_fields[0][1].name" == "company"
jsonpath "$.data[1].collections[0].collection_fields[0][1].type" == "string"
# user's email field (inherited transitively from core)
jsonpath "$.data[2].collections[0].collection_fields[0][1].name" == "email"
jsonpath "$.data[2].collections[0].collection_fields[0][1].type" == "string"

# 4. Unknown site returns error
GET http://127.0.0.1:{{port}}/schema/collections
X-Site-Id: nonexistent
HTTP 404
[Asserts]
jsonpath "$.ok" == false
