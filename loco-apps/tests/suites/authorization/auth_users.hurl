# Person accounts are personal. Org owners manage membership, not identities.
#
# POST /auth/users — self-service signup (no token, password required)
# GET  /auth/me    — authenticated self-read
# GET  /auth/users/list — any authenticated identity (invite directory)
# PUT / DELETE /auth/users/{id} — self only
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

# No token → 401 on /me and list. Signup is the open POST.
GET http://127.0.0.1:{{port}}/auth/me
HTTP 401
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/auth/users/list
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

# Authenticated directory (invite picker). Any logged-in identity, not an org role.
GET http://127.0.0.1:{{port}}/auth/users/list
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data[?(@.username == 'alice')]" count == 1
jsonpath "$.data[?(@.username == 'bob')]" count == 1
jsonpath "$.data[?(@.username == 'dana')]" count == 1

# Identity mutation is self only — not a project editor, not an org owner.
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

PUT http://127.0.0.1:{{port}}/auth/users/{{bob_id}}
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "Robert"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "Robert"

# Org owner still cannot mutate someone else's person account.
POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "staff"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "staff"
jsonpath "$.data.type" == "org"

PUT http://127.0.0.1:{{port}}/auth/users/{{dana_id}}
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "Nope"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

DELETE http://127.0.0.1:{{port}}/auth/users/{{dana_id}}
Authorization: Bearer {{alice_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

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

# Self-delete works.
DELETE http://127.0.0.1:{{port}}/auth/users/{{dana_id}}
Authorization: Bearer {{dana_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# Restore project membership so authorization.hurl can invite bob as editor.
DELETE http://127.0.0.1:{{port}}/config/member/alice/testapp/bob
Authorization: Bearer {{alice_token}}
HTTP 200
