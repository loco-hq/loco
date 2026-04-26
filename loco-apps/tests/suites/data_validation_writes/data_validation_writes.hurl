# Data validation on writes
#
# Fixtures: an "items" collection with fields:
#   name:string, quantity:integer, price:float, in_stock:boolean
#
# Verifies that add/update reject:
#   - unknown fields (kind=unknown_field, severity=error)
#   - type mismatches (kind=type_mismatch, severity=error)
# and that valid records succeed (no diagnostics on the response).

# 1. Baseline: a fully valid insert succeeds and carries no diagnostics
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Widget",
  "quantity": 10,
  "price": 9.99,
  "in_stock": true
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.name" == "Widget"
jsonpath "$.diagnostics" not exists
[Captures]
ok_id: jsonpath "$.data.id"

# 2. Null is allowed for any declared field
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": null,
  "quantity": null
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" not exists

# 3. JSON integer is accepted for a float-declared field
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Gadget",
  "price": 5
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" not exists

# 4. Unknown field — should reject with diagnostic
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Widget",
  "color": "red"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "validation failed"
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "error"
jsonpath "$.diagnostics[0].kind" == "unknown_field"
jsonpath "$.diagnostics[0].path" == "color"

# 5. Type mismatch — string for integer field
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "Widget",
  "quantity": "ten"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.diagnostics[0].path" == "quantity"

# 6. Type mismatch — float for integer field (we DO want this rejected,
# even though the reverse direction is allowed)
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "quantity": 1.5
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.diagnostics[0].path" == "quantity"

# 7. Type mismatch — string "true" for boolean
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "in_stock": "true"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.diagnostics[0].path" == "in_stock"

# 8. Multiple issues in one payload — all are reported
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "ghost": "x",
  "another_ghost": 1,
  "quantity": "not-a-number"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics" count == 3

# 9. Update with unknown field — reject
PUT http://127.0.0.1:{{port}}/data/items/update/{{ok_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "weight": 42
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "unknown_field"
jsonpath "$.diagnostics[0].path" == "weight"

# 10. Update with type mismatch — reject
PUT http://127.0.0.1:{{port}}/data/items/update/{{ok_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "in_stock": 1
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "type_mismatch"

# 11. Valid update — partial patch with declared field
PUT http://127.0.0.1:{{port}}/data/items/update/{{ok_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "quantity": 25
}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.quantity" == 25
jsonpath "$.diagnostics" not exists

# 12. Update on missing record still validates first — bad payload returns
# 400 with diagnostics, NOT 404 (validation runs before lookup).
PUT http://127.0.0.1:{{port}}/data/items/update/no-such-record-id
X-Project-Id: testuser/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "ghost": "x"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "unknown_field"
