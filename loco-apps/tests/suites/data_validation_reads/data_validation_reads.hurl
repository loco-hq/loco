# Data validation on reads
#
# Fixtures: an "items" collection with name:string, quantity:integer, extra:string
#
# Strategy: writes are validated, so we can't directly insert drifted data.
# Instead we write clean records, then mutate the schema (delete a field /
# change a field's type) to simulate the data going stale relative to a
# newer schema. Reads should surface diagnostics as warnings — never fail.

# Login as `testuser` via the `loco/cards` editor site so we have a token
# allowed to mutate `testuser/testapp` schema mid-test.
POST http://127.0.0.1:{{port}}/auth/login
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"username": "testuser"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Insert two clean records
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Widget",
  "quantity": 10,
  "extra": "alpha"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" not exists
[Captures]
rec1_id: jsonpath "$.data.id"

POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Gadget",
  "quantity": 20,
  "extra": "beta"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
[Captures]
rec2_id: jsonpath "$.data.id"

# 2. Get a clean record — no diagnostics
GET http://127.0.0.1:{{port}}/data/items/get/{{rec1_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.id" == {{rec1_id}}
jsonpath "$.diagnostics" not exists

# 3. List clean records — no diagnostics
GET http://127.0.0.1:{{port}}/data/items/list
X-Project-Id: testuser/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.diagnostics" not exists

# 4. Drop the `extra` field from the schema. Existing records still carry
# this field in the lake — it just becomes "unknown" now.
DELETE http://127.0.0.1:{{port}}/schema/testuser/testapp/0-draft/field/items/extra
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 5. Get a record — read still succeeds, but a warning surfaces.
GET http://127.0.0.1:{{port}}/data/items/get/{{rec1_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.extra" == "alpha"
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "warning"
jsonpath "$.diagnostics[0].kind" == "unknown_field"
jsonpath "$.diagnostics[0].path" == "extra"

# 6. List records — both records emit a warning, paths prefixed with record id.
GET http://127.0.0.1:{{port}}/data/items/list
X-Project-Id: testuser/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.diagnostics" count == 2
jsonpath "$.diagnostics[0].severity" == "warning"
jsonpath "$.diagnostics[0].kind" == "unknown_field"
# Paths are "{record_id}/extra" — assert each record_id appears once.
jsonpath "$.diagnostics[*].path" includes "{{rec1_id}}/extra"
jsonpath "$.diagnostics[*].path" includes "{{rec2_id}}/extra"

# 7. Now retype the `quantity` field from integer → string. Existing records
# stored integers; reads should now warn about the type mismatch.
PUT http://127.0.0.1:{{port}}/schema/testuser/testapp/0-draft/field/items/quantity
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "type": "string"
}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.type" == "string"

# 8. Get a record — both warnings present (unknown_field for `extra` AND
# type_mismatch for `quantity`).
GET http://127.0.0.1:{{port}}/data/items/get/{{rec1_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" count == 2
jsonpath "$.diagnostics[*].kind" includes "unknown_field"
jsonpath "$.diagnostics[*].kind" includes "type_mismatch"
jsonpath "$.diagnostics[*].path" includes "quantity"

# 9. Writes against the now-different schema must follow the new types.
# Sending an integer for quantity (now declared string) should 400.
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Trinket",
  "quantity": 5
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.diagnostics[0].path" == "quantity"

# 10. Writes that match the new schema succeed cleanly.
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Trinket",
  "quantity": "5"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" not exists
