# Person accounts are personal. Org owners manage membership, not identities.
#
# POST /auth/users — self-service signup (no token, password required)
# GET  /auth/me    — authenticated self-read
# PUT / DELETE /auth/users/{id} — self only
# GET  /auth/users/list — removed; invite directory is later
#
# Fixture: alice owns alice/testapp, bob owns bob/myapp. This file runs
# before authorization.hurl (alpha order) and must not leave bob as a
# project member of alice/testapp.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"
alice_id: jsonpath "$.data.user.id"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "bob", "password": "password"}
HTTP 200
[Captures]
bob_token: jsonpath "$.data.token"
bob_id: jsonpath "$.data.user.id"

# Self-read
GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.username" == "bob"
jsonpath "$.data.id" == "{{bob_id}}"
jsonpath "$.data.account_type" == "person"

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data.username" == "alice"

# No token → 401 on /me. Directory list is gone.
GET http://127.0.0.1:{{port}}/auth/me
HTTP 401
[Asserts]
jsonpath "$.ok" == false

# /users/{id} is PUT/DELETE only, so GET /users/list is 405 (no directory).
GET http://127.0.0.1:{{port}}/auth/users/list
HTTP 405

# Login of an unknown handle with no password must not stamp TEST_PASSWORD.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "eve"}
HTTP 401
[Asserts]
jsonpath "$.ok" == false

# Self-service signup: no token, password required.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "dana", "name": "Dana"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "password is required"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "dana", "name": "Dana", "password": "password"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.username" == "dana"
jsonpath "$.data.name" == "Dana"
[Captures]
dana_id: jsonpath "$.data.id"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "dana", "password": "password"}
HTTP 200
[Captures]
dana_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{dana_token}}
HTTP 200
[Asserts]
jsonpath "$.data.username" == "dana"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "dana", "name": "Dana", "password": "password"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false

# Identity mutation is self only.
PUT http://127.0.0.1:{{port}}/auth/users/{{alice_id}}
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "Hijacked"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

DELETE http://127.0.0.1:{{port}}/auth/users/{{alice_id}}
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

DELETE http://127.0.0.1:{{port}}/auth/users/{{dana_id}}
Authorization: Bearer {{alice_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

PUT http://127.0.0.1:{{port}}/auth/users/{{bob_id}}
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "Robert"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "Robert"

PUT http://127.0.0.1:{{port}}/auth/users/{{bob_id}}
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "Bob"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "Bob"

# Project editor cannot delete an org owner (the original hole).
POST http://127.0.0.1:{{port}}/config/member/alice/testapp
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201

DELETE http://127.0.0.1:{{port}}/auth/users/{{alice_id}}
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Handle reuse must not inherit the previous identity's memberships.
POST http://127.0.0.1:{{port}}/config/member/alice/testapp
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "dana", "role": "editor"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{dana_token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/auth/users/{{dana_id}}
Authorization: Bearer {{dana_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "dana", "name": "Dana", "password": "pwned"}
HTTP 201

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "dana", "password": "pwned"}
HTTP 200
[Captures]
dana2_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Authorization: Bearer {{dana2_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/config/member/alice/testapp/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.handle == 'dana')]" count == 0

# Restore project membership so authorization.hurl can invite bob as editor.
DELETE http://127.0.0.1:{{port}}/config/member/alice/testapp/bob
Authorization: Bearer {{alice_token}}
HTTP 200
