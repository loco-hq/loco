# Data routes work without auth tokens (SiteId only)
#
# Data routes should remain accessible with just an X-Site-Id header,
# no Bearer token required.

# 1. Add a record — no auth token, just site header
POST http://127.0.0.1:{{port}}/alice/testapp/collection/widgets/add
X-Site-Id: alice-dev
Content-Type: application/json
{
  "fields": {"size": "large"},
  "owner": "testuser"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
[Captures]
record_id: jsonpath "$.data.id"

# 2. List records — no auth token
GET http://127.0.0.1:{{port}}/alice/testapp/collection/widgets/list
X-Site-Id: alice-dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

# 3. Get record — no auth token
GET http://127.0.0.1:{{port}}/alice/testapp/collection/widgets/get/{{record_id}}
X-Site-Id: alice-dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 4. Delete record — no auth token
DELETE http://127.0.0.1:{{port}}/alice/testapp/collection/widgets/delete/{{record_id}}
X-Site-Id: alice-dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
