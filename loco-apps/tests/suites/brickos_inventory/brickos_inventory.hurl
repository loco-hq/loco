# brickos/inventory is committed under schemas/seed/brickos/ and seeded into
# the store on boot. This suite runs against that committed seed.
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
jsonpath "$.data[?(@.name == 'condition')].options[*].value" count == 2
jsonpath "$.data[?(@.name == 'condition')].options[*].value" nth 0 == "new"
jsonpath "$.data[?(@.name == 'condition')].options[*].value" nth 1 == "used"
# The lot key and qty are required; nothing else on a lot is optional yet.
jsonpath "$.data[?(@.required == true)]" count == 5

GET http://127.0.0.1:{{port}}/schema/brickos/inventory/0.0.1-dev/field/batch/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.name == 'kind')].options[*].value" count == 5
jsonpath "$.data[?(@.name == 'kind')].options[*].value" contains "commission"

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

# A lot needs its whole key and a qty. A blank item no. is not one: the
# batch page sends what was typed and shows this diagnostic's message.
POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"batch_id": "{{batch_id}}", "item_no": "", "color_code": 11, "condition": "new", "qty": 1}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "item_no"
jsonpath "$.diagnostics[0].message" == "field 'item_no' is required"

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"item_no": "3001", "color_code": 11, "condition": "new"}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 2
jsonpath "$.diagnostics[*].path" includes "batch_id"
jsonpath "$.diagnostics[*].path" includes "qty"

# A batch needs a label; its item no. is only for a set or minifig.
POST http://127.0.0.1:{{port}}/data/batch/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"label": "", "kind": "inventory", "item_no": ""}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "label"

# Editing one cell sends only that field, so the rest need not be resent;
# clearing a required one is refused.
PUT http://127.0.0.1:{{port}}/data/batch/update/{{batch_id}}
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"label": ""}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "required"

PUT http://127.0.0.1:{{port}}/data/batch/update/{{batch_id}}
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"item_no": "10188-1"}
HTTP 200

# A condition is one of the field's options; a typo is not a new condition.
POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"batch_id": "{{batch_id}}", "item_no": "3001", "color_code": 11, "condition": "nwe", "qty": 1}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "condition"

# A second lot, in another batch, that the batch page must not show.
POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"batch_id": "elsewhere", "item_no": "3001", "color_code": 11, "condition": "used", "qty": 2}
HTTP 201

# The batch page's one query for its lots (`useLots` in the example).
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"queries": {"q": {
  "collection": "lot",
  "where": {"field": "batch_id", "op": "eq", "value": "{{batch_id}}"},
  "order": [{"field": "item_no"}, {"field": "color_code"}, {"field": "condition"}],
  "limit": 500
}}}
HTTP 200
[Asserts]
jsonpath "$.data.q.records" count == 1
jsonpath "$.data.q.records[0].fields.batch_id" == "{{batch_id}}"
jsonpath "$.data.q.cursor" == null

PUT http://127.0.0.1:{{port}}/data/batch/update/{{batch_id}}
Authorization: Bearer {{token}}
X-Project-Id: brickos/inventory
X-Site-Id: dev
Content-Type: application/json
{"kind": "stash"}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "kind"

# --- 3. Inventory is private: public has no permission set.

GET http://127.0.0.1:{{port}}/data/lot/list
X-Project-Id: brickos/inventory
X-Site-Id: dev
HTTP 403
