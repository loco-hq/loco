# Signup tells you what was wrong.
#
# An illegal handle is 400 naming the account-name rule (the same sentence
# as a project name). A 64-character handle is over that cap; a 63-character
# one is accepted. A body of {} is 400 JSON naming `username`, not 422
# text/plain. A taken handle is 409. Login of a wrong password, or of a
# handle that cannot be created, stays 401 invalid credentials.
#
# A 64-character handle already on disk (fixtures/auth) still logs in:
# the cap is checked when a handle is created, not when it is loaded.

# Hyphen is not in the handle charset.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "lego-reseller", "name": "Lego Reseller", "password": "password"}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" == "handle name \"lego-reseller\" must be 1-63 characters of a-z, 0-9, and _, starting with a letter or _"

# That refusal did not create the account. A second login would succeed
# if the first signup had stored it, and auto-create is on in this runner.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "lego-reseller", "password": "password"}
HTTP 401
[Asserts]
jsonpath "$.error" == "invalid credentials"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "lego-reseller", "password": "password"}
HTTP 401
[Asserts]
jsonpath "$.error" == "invalid credentials"

# 64 characters is over the cap. 63 is the longest handle signup accepts.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "name": "Bee", "password": "password"}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" == "handle name \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\" must be 1-63 characters of a-z, 0-9, and _, starting with a letter or _"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc", "name": "Cee", "password": "password"}
HTTP 201
[Asserts]
jsonpath "$.data.username" == "ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc", "password": "password"}
HTTP 200
[Captures]
cee_token: jsonpath "$.data.token"
cee_id: jsonpath "$.data.user.id"

# `public` is a reserved principal, not an account.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "public", "name": "Public", "password": "password"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "handle name \"public\" is reserved"

# Missing fields are JSON 400 naming the field, not axum's 422 text/plain.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "username"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "lego_reseller"}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "name"

# A body that is not JSON is the same envelope.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
`{`
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false

# A taken handle already said so: 409, and the message names the conflict.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "alice", "name": "Alice", "password": "password"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "user already exists"

# Login of a wrong password stays a failed login.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "nope"}
HTTP 401
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "invalid credentials"

# A bad login body names the missing field. It is not 401 and not 422.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "username"

# The 64-character handle in fixtures/auth was not re-checked on load.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "password": "password"}
HTTP 200
[Asserts]
jsonpath "$.data.user.username" == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

# The other /auth bodies that read JSON use the same rejection.
POST http://127.0.0.1:{{port}}/auth/api-keys
Authorization: Bearer {{cee_token}}
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "label"

PUT http://127.0.0.1:{{port}}/auth/users/{{cee_id}}
Authorization: Bearer {{cee_token}}
Content-Type: application/json
`{`
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
