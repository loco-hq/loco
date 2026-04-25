# Authorization integration test
# Verifies: user-scoped access control on schema/config routes
#
# Fixture: alice owns alice/testapp, bob owns bob/myapp
# Both use "cards" site (config-editing site)

# Login as alice
POST http://127.0.0.1:{{port}}/auth/login
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"username": "alice"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

# Login as bob
POST http://127.0.0.1:{{port}}/auth/login
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"username": "bob"}
HTTP 200
[Captures]
bob_token: jsonpath "$.data.token"

# 1. Alice can create a collection on her own project
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{
  "name": "tasks",
  "label": "Task",
  "label_plural": "Tasks"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

# 2. Alice can list collections on her own project (widgets from fixture + tasks just created)
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2

# 3. Bob CANNOT access Alice's schema (403)
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 4. Bob CANNOT create a collection on Alice's project (403)
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{
  "name": "sneaky",
  "label": "Sneaky",
  "label_plural": "Sneakies"
}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 5. Bob CAN access his own project
POST http://127.0.0.1:{{port}}/schema/bob/myapp/0-draft/collection
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{
  "name": "ships",
  "label": "Ship",
  "label_plural": "Ships"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

# 6. Alice CANNOT access Bob's project (403)
GET http://127.0.0.1:{{port}}/schema/bob/myapp/0-draft/collection/list
Authorization: Bearer {{alice_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 7. Config list returns only the user's projects
GET http://127.0.0.1:{{port}}/config/project/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

GET http://127.0.0.1:{{port}}/config/project/list
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

# 8. Bob CANNOT get Alice's config (403)
GET http://127.0.0.1:{{port}}/config/get/project/alice/testapp/project
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 9. No token → 401
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
HTTP 401
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/config/project/list
HTTP 401
[Asserts]
jsonpath "$.ok" == false

# 10. Clean up — delete what we created
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/tasks
Authorization: Bearer {{alice_token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/schema/bob/myapp/0-draft/collection/ships
Authorization: Bearer {{bob_token}}
HTTP 200
