# Credentials are hashed at rest. This file proves the auth flows still work;
# `suite_auth_credentials` in tests/hurl_runner.rs asserts on what landed in
# `auth/` on disk.
#
# POST /auth/users        — signup
# POST /auth/login        — password verified against an argon2 hash
# POST /auth/api-keys     — plaintext key returned once, hash stored
# GET  /auth/api-keys/list — never returns the key
# DELETE /auth/api-keys/{id} — revoked keys stop authenticating

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "dora", "name": "Dora", "password": "correct-horse-battery-staple"}
HTTP 201
[Asserts]
jsonpath "$.data.username" == "dora"

# The signup response never echoes the password back.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "dora", "password": "wrong-horse-battery-staple"}
HTTP 401
[Asserts]
jsonpath "$.error" == "invalid credentials"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "dora", "password": "correct-horse-battery-staple"}
HTTP 200
[Captures]
dora_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{dora_token}}
HTTP 200
[Asserts]
jsonpath "$.data.username" == "dora"

# Seeded identities are hashed the same way, so the documented password holds.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200

# The key is returned once, here.
POST http://127.0.0.1:{{port}}/auth/api-keys
Authorization: Bearer {{dora_token}}
Content-Type: application/json
{"label": "ci"}
HTTP 201
[Captures]
api_key: jsonpath "$.data.key"
api_key_id: jsonpath "$.data.id"
[Asserts]
jsonpath "$.data.label" == "ci"
jsonpath "$.data.key" matches /^[0-9a-f-]{36}$/

# ...and never again.
GET http://127.0.0.1:{{port}}/auth/api-keys/list
Authorization: Bearer {{dora_token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].id" == "{{api_key_id}}"
jsonpath "$.data[0].label" == "ci"
jsonpath "$.data[0].key" not exists

# The plaintext key still authenticates against the stored hash.
GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.username" == "dora"

DELETE http://127.0.0.1:{{port}}/auth/api-keys/{{api_key_id}}
Authorization: Bearer {{dora_token}}
HTTP 200

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{api_key}}
HTTP 401
