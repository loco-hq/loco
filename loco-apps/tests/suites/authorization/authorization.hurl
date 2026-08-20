# Authorization: membership is the ACL.
#
# Fixture: alice owns alice/testapp (person-owned → implicit developer),
# bob owns bob/myapp. loco is an org and cannot log in.

# Login is global — password required, no site headers, AuthUser is not site-scoped.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"
[Asserts]
jsonpath "$.data.user.username" == "alice"
jsonpath "$.data.user.account_type" == "person"
jsonpath "$.data.user.site_id" not exists

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "bob", "password": "password"}
HTTP 200
[Captures]
bob_token: jsonpath "$.data.token"

# Org accounts cannot log in
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "loco", "password": "password"}
HTTP 401

# Wrong password is rejected; omitting password is no longer a bypass
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "wrong"}
HTTP 401

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice"}
HTTP 401

# 1. Alice (developer of alice/testapp) can mutate schema
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

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2

# 2. Bob cannot touch Alice's schema
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

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

# Authenticated Bob also cannot write Alice's data (public hole is token-less only)
POST http://127.0.0.1:{{port}}/data/widgets/add
Authorization: Bearer {{bob_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"size": "tiny"}
HTTP 403

# 3. Alice invites Bob as editor → Bob can /data, cannot /schema
POST http://127.0.0.1:{{port}}/config/member/alice/testapp
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.handle" == "bob"
jsonpath "$.data.role" == "editor"
jsonpath "$.data.pending" == false

POST http://127.0.0.1:{{port}}/data/widgets/add
Authorization: Bearer {{bob_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"size": "medium"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.owner" == "bob"
[Captures]
bob_widget: jsonpath "$.data.id"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{bob_token}}
HTTP 403

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{
  "name": "sneaky",
  "label": "Sneaky",
  "label_plural": "Sneakies"
}
HTTP 403

# 4. Promote Bob to developer → Bob can /schema
PUT http://127.0.0.1:{{port}}/config/member/alice/testapp/bob
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"role": "developer"}
HTTP 200
[Asserts]
jsonpath "$.data.role" == "developer"

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{
  "name": "notes",
  "label": "Note",
  "label_plural": "Notes"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

# 5. Org path: create acme, Alice is owner, Alice creates acme/crm and edits its schema
POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "acme"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.handle" == "acme"
jsonpath "$.data.type" == "org"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{
  "name": "crm",
  "label": "CRM",
  "description": "Acme CRM",
  "account": "acme"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.project" == "acme/crm"

POST http://127.0.0.1:{{port}}/schema/acme/crm/0.0.1-dev/collection
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{
  "name": "accounts",
  "label": "Account",
  "label_plural": "Accounts"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

# Bob is not an acme member
GET http://127.0.0.1:{{port}}/schema/acme/crm/0.0.1-dev/collection/list
Authorization: Bearer {{bob_token}}
HTTP 403

# Config list: Alice sees alice/testapp + acme/crm; Bob sees bob/myapp + alice/testapp (developer)
GET http://127.0.0.1:{{port}}/config/project/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2

GET http://127.0.0.1:{{port}}/config/project/list
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2

# No token → 401 on schema/config
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
HTTP 401
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/config/project/list
HTTP 401
[Asserts]
jsonpath "$.ok" == false

# Clean up
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/notes
Authorization: Bearer {{bob_token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/tasks
Authorization: Bearer {{alice_token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/data/widgets/delete/{{bob_widget}}
Authorization: Bearer {{bob_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
