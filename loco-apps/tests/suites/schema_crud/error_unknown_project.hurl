# Error handling: operations against a non-existent project
# Fixture only has alice/testapp — anything else should 404.

# 1. Create collection on unknown project
POST http://127.0.0.1:{{port}}/schema/nobody/fakething/0-draft/collection
Content-Type: application/json
{
  "name": "widgets",
  "label": "Widget",
  "label_plural": "Widgets"
}
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 2. List collections on unknown project
GET http://127.0.0.1:{{port}}/schema/nobody/fakething/0-draft/collection/list
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 3. Get collection on unknown project
GET http://127.0.0.1:{{port}}/schema/nobody/fakething/0-draft/collection/widgets
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 4. Create field on unknown project
POST http://127.0.0.1:{{port}}/schema/nobody/fakething/0-draft/field/widgets
Content-Type: application/json
{
  "name": "size",
  "type": "integer"
}
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 5. List fields on unknown project
GET http://127.0.0.1:{{port}}/schema/nobody/fakething/0-draft/field/widgets/list
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"
