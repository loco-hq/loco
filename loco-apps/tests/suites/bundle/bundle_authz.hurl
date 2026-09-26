# Bundle authz: writing a bundle is a /schema write (developer), reading its
# metadata is a /schema read (developer or editor). Runs after bundle.hurl,
# which leaves 0-boot-dev with a tree and 0-dev with none.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "bob", "password": "password"}
HTTP 200
[Captures]
bob_token: jsonpath "$.data.token"

# 1. No token at all: a bundle is not public through /schema.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/bundle
Content-Type: application/zip
file,dist.zip;
HTTP 401

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
HTTP 401

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
HTTP 401

# 2. A non-member cannot read or write it either.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
Authorization: Bearer {{bob_token}}
HTTP 403

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/bundle
Authorization: Bearer {{bob_token}}
Content-Type: application/zip
file,dist.zip;
HTTP 403

# 3. Alice invites Bob as editor: he can read the metadata, not deploy.
POST http://127.0.0.1:{{port}}/config/member/alice/testapp
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.data.files" == 2

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/bundle
Authorization: Bearer {{bob_token}}
Content-Type: application/zip
file,dist.zip;
HTTP 403

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
Authorization: Bearer {{bob_token}}
HTTP 403

# The refused PUT wrote nothing.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/bundle
Authorization: Bearer {{alice_token}}
HTTP 404

# 4. Promote Bob to developer: deploy is his now.
PUT http://127.0.0.1:{{port}}/config/member/alice/testapp/bob
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"role": "developer"}
HTTP 200

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/bundle
Authorization: Bearer {{bob_token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
[Asserts]
jsonpath "$.data.files" == 2
jsonpath "$.data.size" == 56

# 5. Deleting the version takes its bundle with it — a recreated version does
#    not inherit the deleted one's frontend.
DELETE http://127.0.0.1:{{port}}/config/version/alice/testapp/0-boot-dev
Authorization: Bearer {{alice_token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
Authorization: Bearer {{alice_token}}
HTTP 404

POST http://127.0.0.1:{{port}}/config/version/alice/testapp
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"version": "0-boot-dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot-dev/bundle
Authorization: Bearer {{alice_token}}
HTTP 404
