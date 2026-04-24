# Error handling: bad or missing site ID
# Fixtures provide a valid collection (tasks) and site (dev),
# so these errors are specifically about site resolution, not missing collections.

# 1. Missing X-Site-Id header entirely
GET http://127.0.0.1:{{port}}/data/tasks/list
X-Project-Id: testuser/testapp
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "missing site"

# 2. Unknown site ID on list
GET http://127.0.0.1:{{port}}/data/tasks/list
X-Project-Id: testuser/testapp
X-Site-Id: nonexistent
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown site"

# 3. Unknown site ID on add
POST http://127.0.0.1:{{port}}/data/tasks/add
X-Project-Id: testuser/testapp
X-Site-Id: nonexistent
Content-Type: application/json
{
  "fields": {
    "title": "Should fail"
  }
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown site"

# 4. Unknown site ID on get
GET http://127.0.0.1:{{port}}/data/tasks/get/fake-id
X-Project-Id: testuser/testapp
X-Site-Id: nonexistent
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown site"

# 5. Unknown site ID on update
PUT http://127.0.0.1:{{port}}/data/tasks/update/fake-id
X-Project-Id: testuser/testapp
X-Site-Id: nonexistent
Content-Type: application/json
{
  "fields": {
    "title": "Should fail"
  }
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown site"

# 6. Unknown site ID on delete
DELETE http://127.0.0.1:{{port}}/data/tasks/delete/fake-id
X-Project-Id: testuser/testapp
X-Site-Id: nonexistent
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown site"

# 7. Empty site ID header — should be treated as missing
GET http://127.0.0.1:{{port}}/data/tasks/list
X-Project-Id: testuser/testapp
X-Site-Id:
HTTP 400
[Asserts]
jsonpath "$.ok" == false
