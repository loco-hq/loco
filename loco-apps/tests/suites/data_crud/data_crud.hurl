# Data CRUD integration test
# Fixtures: a "tasks" collection with "title" (string) and "done" (boolean) fields,
#           a "dev" site pointing to namespace "testuser/testapp"
#
# Tests: add records, list, get by id, update, delete

# 1. Verify the fixture collection loaded — list should work (empty)
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/list
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0

# 2. Add a record
POST http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/add
X-Site-Id: dev
Content-Type: application/json
{
  "fields": {
    "title": "Write tests",
    "done": false
  },
  "owner": "alice"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.id" isString
jsonpath "$.data.owner" == "alice"
jsonpath "$.data.fields.title" == "Write tests"
jsonpath "$.data.fields.done" == false
[Captures]
record1_id: jsonpath "$.data.id"

# 3. Add a second record
POST http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/add
X-Site-Id: dev
Content-Type: application/json
{
  "fields": {
    "title": "Ship feature",
    "done": false
  }
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.title" == "Ship feature"
[Captures]
record2_id: jsonpath "$.data.id"

# 4. List records — should have 2
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/list
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2

# 5. Get the first record by ID
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/get/{{record1_id}}
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.id" == {{record1_id}}
jsonpath "$.data.fields.title" == "Write tests"

# 6. Update the first record
PUT http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/update/{{record1_id}}
X-Site-Id: dev
Content-Type: application/json
{
  "fields": {
    "done": true
  }
}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.done" == true
jsonpath "$.data.fields.title" == "Write tests"

# 7. Get the updated record — verify fields merged
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/get/{{record1_id}}
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.done" == true
jsonpath "$.data.fields.title" == "Write tests"

# 8. Delete the second record
DELETE http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/delete/{{record2_id}}
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 9. List records — should have 1
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/list
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

# 10. Get deleted record — should 404
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/get/{{record2_id}}
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.ok" == false

# 11. Verify missing site header returns error
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/tasks/list
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "missing site"

# 12. Verify unknown collection returns 404
GET http://127.0.0.1:{{port}}/testuser/testapp/collection/nonexistent/list
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.ok" == false
