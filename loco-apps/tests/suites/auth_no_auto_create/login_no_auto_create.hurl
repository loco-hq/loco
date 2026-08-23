# Auto-create is off (the production default). Login of an unknown handle is
# a 401, not a free person account — owning `acme` would imply developer on
# every `acme/*` project.
#
# This suite runs with `auth_auto_create: Some(false)` even though the rest
# of the Hurl runner sets LOCO_AUTH_AUTO_CREATE=1.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "acme", "password": "password"}
HTTP 401
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "invalid credentials"

# Still nothing to log in as on a second try.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "acme", "password": "password"}
HTTP 401

# Seeded identities are unaffected.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data.username" == "alice"

# Signup is the supported way in — and it is explicit.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "carol", "name": "Carol", "password": "password"}
HTTP 201
[Asserts]
jsonpath "$.data.username" == "carol"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "carol", "password": "password"}
HTTP 200
[Asserts]
jsonpath "$.data.user.username" == "carol"
