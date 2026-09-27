# brickos/inventory is committed under schemas/instances/brickos/ and loads on
# boot. This suite runs against that committed tree, not a copy of it.
#
# The `brickos` org is not seeded; whoever creates it owns it, and an org
# owner is a developer on every project under it.

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

# --- 1. The five collections load.

GET http://127.0.0.1:{{port}}/schema/brickos/inventory/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 5
jsonpath "$.data[*].name" contains "item_type"
jsonpath "$.data[*].name" contains "color"
jsonpath "$.data[*].name" contains "item"
jsonpath "$.data[*].name" contains "batch"
jsonpath "$.data[*].name" contains "lot"

GET http://127.0.0.1:{{port}}/schema/brickos/inventory/0.0.1-dev/field/lot/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 5
jsonpath "$.data[?(@.name == 'batch_id')].type" nth 0 == "string"
jsonpath "$.data[?(@.name == 'color_code')].type" nth 0 == "integer"
jsonpath "$.data[?(@.name == 'qty')].type" nth 0 == "integer"

# --- 2. A batch and one of its lots, through ordinary record CRUD on `dev`.

POST http://127.0.0.1:{{port}}/data/batch/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"label": "On hand", "kind": "inventory", "item_no": ""}
HTTP 201
[Captures]
batch_id: jsonpath "$.data.id"

# 10 black 2x4 bricks, new. The lot names a part with no catalog row, which
# this schema allows.
POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"batch_id": "{{batch_id}}", "item_no": "3001", "color_code": 11, "condition": "new", "qty": 10}
HTTP 201
[Asserts]
jsonpath "$.data.fields.batch_id" == "{{batch_id}}"
jsonpath "$.data.fields.color_code" == 11
jsonpath "$.data.fields.qty" == 10

# The lot key's integers are integers.
POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"batch_id": "{{batch_id}}", "item_no": "3001", "color_code": 11, "condition": "new", "qty": "ten"}
HTTP 400

# --- 3. Inventory is private: public has no permission set.

GET http://127.0.0.1:{{port}}/data/lot/list
X-Project-Id: brickos/inventory
X-Site-Id: dev
HTTP 403
