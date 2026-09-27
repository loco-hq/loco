# A token that fails to authenticate is a 401, not a fall back to `public`.
#
# Only a request with no `Authorization` header at all is `public`. One that
# presents a header — malformed, an unknown or logged-out session, a revoked
# API key — is told its credentials are dead, so a client can tell "sign in
# again" from "you may not do this". (An expired session takes the same path
# as a logged-out one: `validate_session` fails and so does the API-key
# lookup. Sessions are swept at boot, so a fixture cannot hold one.)
#
# Site `www` pins alice/notes@1-dev, whose manifest assigns `public_notes`
# (read + create on `note`) to public.

# --- No header: public, and the grant applies.

GET http://127.0.0.1:{{port}}/data/note/list
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.ok" == true

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: alice/notes
X-Site-Id: www
{"queries": {"c": {"collection": "note"}}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

GET http://127.0.0.1:{{port}}/schema/alice/notes/1-dev/collection/list
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# --- An unknown token: 401 on each of those.

GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Bearer not-a-real-token
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "invalid or expired session"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer not-a-real-token
X-Project-Id: alice/notes
X-Site-Id: www
{"queries": {"c": {"collection": "note"}}}
HTTP 401
[Asserts]
jsonpath "$.error" == "invalid or expired session"

GET http://127.0.0.1:{{port}}/schema/alice/notes/1-dev/collection/list
Authorization: Bearer not-a-real-token
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401
[Asserts]
jsonpath "$.error" == "invalid or expired session"

# A write the public grant would allow is refused too: a dead token never
# acts as `public`.
POST http://127.0.0.1:{{port}}/data/note/add
Authorization: Bearer not-a-real-token
X-Project-Id: alice/notes
X-Site-Id: www
{"body": "boo"}
HTTP 401

# --- A header that is not `Bearer <token>`: 401, and it says why.

GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Basic YWxpY2U6cGFzc3dvcmQ=
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401
[Asserts]
jsonpath "$.error" == "malformed Authorization header: use Bearer <token>"

GET http://127.0.0.1:{{port}}/schema/alice/notes/1-dev/collection/list
Authorization: Bearer
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401
[Asserts]
jsonpath "$.error" == "malformed Authorization header: use Bearer <token>"

# --- A session that has ended.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
session: jsonpath "$.data.token"

# Live, it is alice — not public — on the same site.
GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Bearer {{session}}
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 200

POST http://127.0.0.1:{{port}}/auth/logout
Authorization: Bearer {{session}}
HTTP 200

GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Bearer {{session}}
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{session}}
X-Project-Id: alice/notes
X-Site-Id: www
{"queries": {"c": {"collection": "note"}}}
HTTP 401

GET http://127.0.0.1:{{port}}/schema/alice/notes/1-dev/collection/list
Authorization: Bearer {{session}}
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401

# --- A revoked API key.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/auth/api-keys
Authorization: Bearer {{alice}}
Content-Type: application/json
{"label": "stale"}
HTTP 201
[Captures]
api_key: jsonpath "$.data.key"
api_key_id: jsonpath "$.data.id"

GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Bearer {{api_key}}
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 200

DELETE http://127.0.0.1:{{port}}/auth/api-keys/{{api_key_id}}
Authorization: Bearer {{alice}}
HTTP 200

GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Bearer {{api_key}}
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{api_key}}
X-Project-Id: alice/notes
X-Site-Id: www
{"queries": {"c": {"collection": "note"}}}
HTTP 401

GET http://127.0.0.1:{{port}}/schema/alice/notes/1-dev/collection/list
Authorization: Bearer {{api_key}}
X-Project-Id: alice/notes
X-Site-Id: www
HTTP 401
