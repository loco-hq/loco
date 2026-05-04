# Error handling: operations against a non-existent project
# Fixture only has alice/testapp — anything else should 404.
# Note: alice can't access nobody/fakething because of auth (403),
# so we test with alice's own namespace but a non-existent project.

# Login as alice
POST http://127.0.0.1:{{port}}/auth/login
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"username": "alice"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Create collection on unknown project (alice's namespace, but project doesn't exist)
POST http://127.0.0.1:{{port}}/schema/alice/nonexistent/0-draft/collection
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
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
GET http://127.0.0.1:{{port}}/schema/alice/nonexistent/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 3. Get collection on unknown project
GET http://127.0.0.1:{{port}}/schema/alice/nonexistent/0-draft/collection/widgets
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 4. Create field on unknown project
POST http://127.0.0.1:{{port}}/schema/alice/nonexistent/0-draft/field/widgets
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
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
GET http://127.0.0.1:{{port}}/schema/alice/nonexistent/0-draft/field/widgets/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown project"

# 6. Accessing another user's namespace is forbidden
GET http://127.0.0.1:{{port}}/schema/nobody/fakething/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 403
[Asserts]
jsonpath "$.ok" == false
