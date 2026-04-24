# Data routes work without auth tokens (project + site headers only)
#
# Data routes should remain accessible with just X-Project-Id and X-Site-Id headers,
# no Bearer token required.

# 1. Add a record — no auth token, just project + site headers
POST http://127.0.0.1:{{port}}/data/widgets/add
X-Project-Id: alice/testapp
X-Site-Id: dev
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
GET http://127.0.0.1:{{port}}/data/widgets/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

# 3. Get record — no auth token
GET http://127.0.0.1:{{port}}/data/widgets/get/{{record_id}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 4. Delete record — no auth token
DELETE http://127.0.0.1:{{port}}/data/widgets/delete/{{record_id}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
