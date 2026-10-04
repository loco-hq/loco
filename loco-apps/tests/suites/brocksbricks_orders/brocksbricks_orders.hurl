# brocksbricks/orders (issue #105) is committed under schemas/seed/brocksbricks/
# and seeded into the store on boot, beside loco/bricklink@1.0.0 from the loco
# seed account. This suite runs against both committed seeds.
#
# The `brocksbricks` org is not seeded; whoever creates it owns it, and an org
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
{"handle": "brocksbricks"}
HTTP 201

# --- 1. The store's five collections, then the package's two through `store`.

GET http://127.0.0.1:{{port}}/schema/brocksbricks/orders/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 7
jsonpath "$.data[?(@.project == 'brocksbricks/orders')]" count == 5
jsonpath "$.data[?(@.project == 'brocksbricks/orders')].name" contains "location"
jsonpath "$.data[?(@.project == 'brocksbricks/orders')].name" contains "settings"
jsonpath "$.data[?(@.project == 'brocksbricks/orders')].name" contains "pull_run"
jsonpath "$.data[?(@.project == 'brocksbricks/orders')].name" contains "pick_line"
jsonpath "$.data[?(@.project == 'brocksbricks/orders')].name" contains "pick_allocation"
jsonpath "$.data[?(@.integration == 'store')]" count == 2
jsonpath "$.data[?(@.integration == 'store')].project" nth 0 == "loco/bricklink"
jsonpath "$.data[?(@.integration == 'store')].owner" nth 0 == "loco/bricklink"
jsonpath "$.data[?(@.integration == 'store')].name" nth 0 == "orders"
jsonpath "$.data[?(@.integration == 'store')].name" nth 1 == "order_items"

# The dev site is the one the project ships, pinned to 0.0.1-dev.
GET http://127.0.0.1:{{port}}/config/site/brocksbricks/orders/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

# A pick line's fields come back in the default fieldset's order, so Studio's
# columns read station, location, then the lot.
GET http://127.0.0.1:{{port}}/data/pick_line/fields
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 12
jsonpath "$.data[0].name" == "pull_run_id"
jsonpath "$.data[1].name" == "station"
jsonpath "$.data[?(@.name == 'station')].options[*].value" count == 3
jsonpath "$.data[?(@.name == 'station')].options[*].value" contains "unlocated"
jsonpath "$.data[?(@.name == 'qty')].type" nth 0 == "integer"
jsonpath "$.data[?(@.name == 'walk_order')].type" nth 0 == "integer"
jsonpath "$.data[?(@.name == 'color_id')].type" nth 0 == "integer"
# Condition is copied from order_items as BrickLink spells it.
jsonpath "$.data[?(@.name == 'condition')].options[*].value" contains "N"
jsonpath "$.data[?(@.name == 'condition')].options[*].value" contains "U"
# An unlocated line has no location or walk order, and nothing is picked yet.
jsonpath "$.data[?(@.required == true)]" count == 7

GET http://127.0.0.1:{{port}}/data/pick_allocation/fields
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 6
jsonpath "$.data[?(@.required == true)].name" contains "bl_order_id"
jsonpath "$.data[?(@.required == true)].name" contains "order_item_id"
jsonpath "$.data[?(@.required == true)].name" not contains "inventory_id"

# The package's collections resolve on the dev site. Nothing is configured, so
# a read stops at the four secrets before any call.
GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/list
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
HTTP 400
[Asserts]
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.diagnostics" count == 4

# --- 2. A pull run and one of its pick lines, through record CRUD on `dev`.

POST http://127.0.0.1:{{port}}/data/pull_run/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"label": "Monday", "created_at": "2026-10-05T08:00:00Z", "status": "open"}
HTTP 201
[Captures]
run_id: jsonpath "$.data.id"

POST http://127.0.0.1:{{port}}/data/pick_line/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "station": "scale", "item_no": "3001", "item_type": "PART", "color_id": 11, "condition": "N", "location_code": "A-01-3", "walk_order": 40, "qty": 250}
HTTP 201
[Captures]
line_id: jsonpath "$.data.id"
[Asserts]
jsonpath "$.data.fields.qty" == 250
jsonpath "$.data.fields.walk_order" == 40

GET http://127.0.0.1:{{port}}/data/pick_line/get/{{line_id}}
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.fields.qty" == 250
jsonpath "$.data.fields.station" == "scale"
jsonpath "$.diagnostics" not exists

# Tapping the line sets picked_qty and nothing else.
PUT http://127.0.0.1:{{port}}/data/pick_line/update/{{line_id}}
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"picked_qty": 250}
HTTP 200
[Asserts]
jsonpath "$.data.fields.picked_qty" == 250
jsonpath "$.data.fields.qty" == 250

POST http://127.0.0.1:{{port}}/data/pick_allocation/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "pick_line_id": "{{line_id}}", "bl_order_id": "29471234", "order_item_id": "29471234-1-901", "inventory_id": "901", "qty": 200}
HTTP 201

# A second scale line earlier on the walk, and a hand line earlier still, so
# the scale view below has an order to get right and a line to leave out.
POST http://127.0.0.1:{{port}}/data/pick_line/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "station": "scale", "item_no": "3002", "item_type": "PART", "color_id": 1, "condition": "N", "location_code": "A-00-1", "walk_order": 10, "qty": 80}
HTTP 201

POST http://127.0.0.1:{{port}}/data/pick_line/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "station": "hand", "item_no": "3004", "item_type": "PART", "color_id": 1, "condition": "N", "location_code": "A-00-0", "walk_order": 5, "qty": 4}
HTTP 201

# #106's scale view in one batch: the run's scale lines in walk order, and the
# run's whole per-order split by pull_run_id alone.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "scale": {
      "collection": "pick_line",
      "where": { "and": [
        { "field": "pull_run_id", "op": "eq", "value": "{{run_id}}" },
        { "field": "station", "op": "eq", "value": "scale" }
      ] },
      "order": [{ "field": "walk_order" }]
    },
    "split": {
      "collection": "pick_allocation",
      "where": { "field": "pull_run_id", "op": "eq", "value": "{{run_id}}" }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.scale.error" not exists
jsonpath "$.data.scale.records" count == 2
jsonpath "$.data.scale.records[0].fields.walk_order" == 10
jsonpath "$.data.scale.records[1].fields.walk_order" == 40
jsonpath "$.data.scale.records[1].id" == "{{line_id}}"
jsonpath "$.data.split.error" not exists
jsonpath "$.data.split.records" count == 1
jsonpath "$.data.split.records[0].fields.pick_line_id" == "{{line_id}}"
jsonpath "$.data.split.records[0].fields.bl_order_id" == "29471234"
jsonpath "$.data.split.records[0].fields.qty" == 200

# A station is one of three; a typo is not a fourth route.
POST http://127.0.0.1:{{port}}/data/pick_line/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "station": "bench", "item_no": "3001", "item_type": "PART", "color_id": 11, "condition": "N", "qty": 3}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "station"

# Quantities are integers, not counted strings.
POST http://127.0.0.1:{{port}}/data/pick_line/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "station": "hand", "item_no": "3001", "item_type": "PART", "color_id": 11, "condition": "N", "qty": "3"}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.diagnostics[0].path" == "qty"

# An unlocated line is still a line: no location, no walk order.
POST http://127.0.0.1:{{port}}/data/pick_line/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"pull_run_id": "{{run_id}}", "station": "unlocated", "item_no": "3003", "item_type": "PART", "color_id": 5, "condition": "U", "remarks": "behind the desk", "qty": 2}
HTTP 201

# --- 3. Location and settings.

POST http://127.0.0.1:{{port}}/data/location/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"code": "A-01-3", "zone": "A", "walk_order": 40}
HTTP 201

POST http://127.0.0.1:{{port}}/data/location/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"code": "A-01-4", "zone": "A"}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "walk_order"

POST http://127.0.0.1:{{port}}/data/settings/add
Authorization: Bearer {{token}}
X-Project-Id: brocksbricks/orders
X-Site-Id: dev
Content-Type: application/json
{"scale_threshold": 50}
HTTP 201
[Asserts]
jsonpath "$.data.fields.scale_threshold" == 50
