# Org create and member add name a bad handle. Login does not.
#
# Auto-create is off. A wrong password, an unknown user, an org handle,
# and an illegal handle are one 401 body. A handle already on disk with a
# hyphen (fixtures/auth, lego-reseller) still logs in: load and login do
# not re-check the charset.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "nope"}
HTTP 401
[Captures]
denied: body

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "nobody", "password": "password"}
HTTP 401
[Asserts]
body == "{{denied}}"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "loco", "password": "password"}
HTTP 401
[Asserts]
body == "{{denied}}"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "my-org", "password": "password"}
HTTP 401
[Asserts]
body == "{{denied}}"

# A second try would succeed if the first had stored the handle.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "my-org", "password": "password"}
HTTP 401
[Asserts]
body == "{{denied}}"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

# An illegal org handle is the signup sentence, and the org is not stored.
POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "my-org"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "handle name \"my-org\" must be 1-63 characters of a-z, 0-9, and _, starting with a letter or _"

# Stored with alice as owner, this would be 201. Stored with no owner, 403.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Must not land", "account": "my-org"}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown account: my-org"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "public"}
HTTP 400
[Asserts]
jsonpath "$.error" == "handle name \"public\" is reserved"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
HTTP 400
[Asserts]
jsonpath "$.error" == "handle name \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\" must be 1-63 characters of a-z, 0-9, and _, starting with a letter or _"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Must not land", "account": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown account: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "acme"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "acme"
jsonpath "$.data.type" == "org"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "acme"}
HTTP 409
[Asserts]
jsonpath "$.error" == "user already exists"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Alice's shop"}
HTTP 201

# The creator's membership is a real row.
GET http://127.0.0.1:{{port}}/config/member/alice/shop/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.handle == 'alice')].role" nth 0 == "developer"
jsonpath "$.data[?(@.handle == 'alice')].pending" nth 0 == false

# Member add: illegal is 400 with the charset sentence and no length cap.
POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bad-handle", "role": "editor"}
HTTP 400
[Asserts]
jsonpath "$.error" == "handle name \"bad-handle\" must be a-z, 0-9, and _, starting with a letter or _"
jsonpath "$.error" not contains "1-63"

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bad-handle", "role": "member"}
HTTP 400
[Asserts]
jsonpath "$.error" == "handle name \"bad-handle\" must be a-z, 0-9, and _, starting with a letter or _"

POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "public", "role": "editor"}
HTTP 400
[Asserts]
jsonpath "$.error" == "handle name \"public\" is reserved"

# A well-formed handle that names no account is 404. Signup's 409
# `user already exists` (no token, for a handle that passes the signup
# rule) and POST /config/project with "account" (404 unknown account or
# 403 for any authenticated caller) already answer whether a handle
# exists, including a legacy or over-63 handle.
POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "nobody", "role": "editor"}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown account: nobody"

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "nobody", "role": "member"}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown account: nobody"

# Over 63 and missing: the cap is not applied, so this is 404, not 400.
POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc", "role": "editor"}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown account: cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "bob"
jsonpath "$.data.role" == "editor"
jsonpath "$.data.pending" == false

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "member"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "bob"
jsonpath "$.data.pending" == false

# A hyphenated person already on disk logs in. The account is accepted
# whatever its charset, so they can be invited and can create a project
# and an org.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "lego-reseller", "password": "password"}
HTTP 200
[Captures]
legacy_token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "lego-reseller", "role": "editor"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "lego-reseller"
jsonpath "$.data.role" == "editor"
jsonpath "$.data.pending" == false

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "lego-reseller", "role": "member"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "lego-reseller"
jsonpath "$.data.pending" == false

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{legacy_token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Legacy shop"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "Shop"

GET http://127.0.0.1:{{port}}/config/member/lego-reseller/shop/list
Authorization: Bearer {{legacy_token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.handle == 'lego-reseller')].role" nth 0 == "developer"
jsonpath "$.data[?(@.handle == 'lego-reseller')].pending" nth 0 == false

GET http://127.0.0.1:{{port}}/config/project/lego-reseller/shop
Authorization: Bearer {{legacy_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "Shop"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{legacy_token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Legacy shop"}
HTTP 409
[Asserts]
jsonpath "$.error" contains "already exists"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{legacy_token}}
Content-Type: application/json
{"handle": "reseller"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "reseller"
jsonpath "$.data.type" == "org"

GET http://127.0.0.1:{{port}}/config/org/reseller/member/list
Authorization: Bearer {{legacy_token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.handle == 'lego-reseller')].role" nth 0 == "owner"
jsonpath "$.data[?(@.handle == 'lego-reseller')].pending" nth 0 == false

# The name is taken.
POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "reseller"}
HTTP 409
[Asserts]
jsonpath "$.error" == "user already exists"
