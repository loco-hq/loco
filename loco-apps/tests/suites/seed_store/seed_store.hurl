# Committed projects live in schemas/seed/ and are copied into the gitignored
# store (schemas/instances/) at boot. Writes to a seeded project go to the
# store only; the runner then checks the seed on disk is byte-for-byte what it
# started as, and that a project deleted from the store comes back on reboot.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{token}}
Content-Type: application/json
{"handle": "brickos"}
HTTP 201

# 1. The seeded project loaded.
GET http://127.0.0.1:{{port}}/schema/brickos/inventory/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 5

# 2. A /schema write to it.
POST http://127.0.0.1:{{port}}/schema/brickos/inventory/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "lot", "name": "note", "type": "string"}
HTTP 201

# 3. A bundle deploy to it.
PUT http://127.0.0.1:{{port}}/schema/brickos/inventory/0.0.1-dev/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
[Asserts]
jsonpath "$.data.files" == 2
