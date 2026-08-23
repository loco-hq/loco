# POST /auth/users is self-service signup (no token, password required).
# List/update/delete stay org-owner admin. Self-read is GET /auth/me.
# Person-account ownership (alice → alice/*) is not an org role.
#
# Fixture: alice owns alice/testapp, bob owns bob/myapp. Neither starts as
# an org owner. This file runs before authorization.hurl (alpha order) and
# must not leave bob as a project member of alice/testapp.

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

# Self-read: any authenticated identity can see themselves.
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

# Logged-in person who does not own an org cannot list, update, or delete.
GET http://127.0.0.1:{{port}}/auth/users/list
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

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

# Self-update is also org-owner-only; /me is the self exception, not mutation.
PUT http://127.0.0.1:{{port}}/auth/users/{{bob_id}}
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "Robert"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Alice is a person-account owner, not an org owner yet.
GET http://127.0.0.1:{{port}}/auth/users/list
Authorization: Bearer {{alice_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Creating an org makes Alice an owner → she can administer identities.
POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "staff"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "staff"
jsonpath "$.data.type" == "org"

GET http://127.0.0.1:{{port}}/auth/users/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data[?(@.username == 'alice')]" count == 1
jsonpath "$.data[?(@.username == 'bob')]" count == 1

POST http://127.0.0.1:{{port}}/auth/users
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"username": "carol", "name": "Carol", "password": "password"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.username" == "carol"
jsonpath "$.data.name" == "Carol"
[Captures]
carol_id: jsonpath "$.data.id"

PUT http://127.0.0.1:{{port}}/auth/users/{{carol_id}}
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "Caroline"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "Caroline"

# Org member (not owner) still cannot administer users.
POST http://127.0.0.1:{{port}}/config/org/staff/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "member"}
HTTP 201

GET http://127.0.0.1:{{port}}/auth/users/list
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

DELETE http://127.0.0.1:{{port}}/auth/users/{{carol_id}}
Authorization: Bearer {{bob_token}}
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

GET http://127.0.0.1:{{port}}/auth/users/list
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

PUT http://127.0.0.1:{{port}}/auth/users/{{alice_id}}
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "Nope"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Org owner can delete another identity.
DELETE http://127.0.0.1:{{port}}/auth/users/{{carol_id}}
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# Restore project membership so authorization.hurl can invite bob as editor.
DELETE http://127.0.0.1:{{port}}/config/member/alice/testapp/bob
Authorization: Bearer {{alice_token}}
HTTP 200
