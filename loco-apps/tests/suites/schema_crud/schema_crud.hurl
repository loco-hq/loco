# Schema CRUD integration test
# Verifies: create collection, list, create field, list fields, get collection

# 1. Create a collection
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Content-Type: application/json
{
  "name": "tasks",
  "label": "Task",
  "label_plural": "Tasks"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "tasks"
jsonpath "$.data.label" == "Task"
jsonpath "$.data.label_plural" == "Tasks"

# 2. List collections — should have exactly 1
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

# 3. Get the specific collection by name
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/tasks
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "tasks"

# 4. Create a field on that collection
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/tasks
Content-Type: application/json
{
  "name": "title",
  "type": "string"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "title"
jsonpath "$.data.collection" == "tasks"
jsonpath "$.data.type" == "string"

# 5. Create a second field
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/tasks
Content-Type: application/json
{
  "name": "done",
  "type": "boolean"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "done"

# 6. List fields for the collection — should have 2
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/tasks/list
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2

# 7. Delete a field
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/tasks/done
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 8. List fields again — should have 1
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/tasks/list
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

# 9. Delete the collection
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/tasks
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 10. List collections — should be empty
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0
