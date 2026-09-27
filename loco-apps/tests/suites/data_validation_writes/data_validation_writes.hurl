# Data validation on writes
#
# Fixtures: an "items" collection with fields:
#   name:string, quantity:integer, price:float, in_stock:boolean,
#   size:string with options s / m / l
# and an "orders" collection whose fields are required:
#   ref:string, qty:integer, paid:boolean, status:string with options
#   open / shipped — and one optional note:string
#
# Verifies that add/update reject:
#   - unknown fields (kind=unknown_field, severity=error)
#   - type mismatches (kind=type_mismatch, severity=error)
#   - a value outside a string field's options (kind=invalid_option)
#   - a required field left missing, null, or "" (kind=required)
# and that valid records succeed (no diagnostics on the response).
# Collection is private, so these run as the project owner.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "testuser", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Baseline: a fully valid insert succeeds and carries no diagnostics
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
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
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ghost": "x"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "unknown_field"

# 13. /schema field reads return a field's options unchanged; a field
# without options returns an empty list.
GET http://127.0.0.1:{{port}}/schema/testuser/testapp/0-dev/field/items/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.name == 'size')].options[*].value" count == 3
jsonpath "$.data[?(@.name == 'size')].options[*].value" nth 0 == "s"
jsonpath "$.data[?(@.name == 'size')].options[*].label" nth 0 == "Small"
jsonpath "$.data[?(@.name == 'size')].options[*].value" nth 2 == "l"
jsonpath "$.data[?(@.name == 'name')].options[*]" isEmpty

# 14. A listed option is accepted
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "Shirt",
  "size": "m"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.size" == "m"
jsonpath "$.diagnostics" not exists
[Captures]
shirt_id: jsonpath "$.data.id"

# 15. An unlisted value is rejected, naming the field
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "Shirt",
  "size": "xl"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "error"
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "size"
jsonpath "$.diagnostics[0].message" contains "xl"

# 16. Options match the stored value exactly: a label is not a value, and
# the empty string is not an option.
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "size": "Medium"
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "invalid_option"

POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "size": ""
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "invalid_option"

# 17. Null still passes for a field with options
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "Blank",
  "size": null
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" not exists

# 18. A wrong type on a field with options is a type_mismatch, not also an
# invalid_option
POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "size": 3
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "type_mismatch"

# 19. Update checks options too
PUT http://127.0.0.1:{{port}}/data/items/update/{{shirt_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "size": "xxl"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "size"

PUT http://127.0.0.1:{{port}}/data/items/update/{{shirt_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "size": "l"
}
HTTP 200
[Asserts]
jsonpath "$.data.fields.size" == "l"
jsonpath "$.diagnostics" not exists

# 20. /schema field reads carry required; a field that never set it is false
GET http://127.0.0.1:{{port}}/schema/testuser/testapp/0-dev/field/orders/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.name == 'ref')].required" nth 0 == true
jsonpath "$.data[?(@.name == 'note')].required" nth 0 == false

# 21. A create that gives every required field a value succeeds. false and 0
# are values.
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "A-1",
  "qty": 0,
  "paid": false,
  "status": "open"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" not exists
[Captures]
order_id: jsonpath "$.data.id"

# 22. A create that omits a required field is rejected, naming the field
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "qty": 1,
  "paid": true,
  "status": "open"
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "validation failed"
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "error"
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "ref"
jsonpath "$.diagnostics[0].message" == "field 'ref' is required"

# 23. null does not satisfy required, for any type
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": null,
  "qty": null,
  "paid": null,
  "status": "open"
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 3
jsonpath "$.diagnostics[*].kind" includes "required"
jsonpath "$.diagnostics[?(@.kind == 'required')].path" includes "ref"
jsonpath "$.diagnostics[?(@.kind == 'required')].path" includes "qty"
jsonpath "$.diagnostics[?(@.kind == 'required')].path" includes "paid"

# 24. Nor does "" for a string field — it is what a cleared text input sends
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "",
  "qty": 1,
  "paid": true,
  "status": "open"
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "ref"

# 25. A required field with options must be one of them: blank is required
# (not also invalid_option), and an unlisted value is invalid_option
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "A-2",
  "qty": 1,
  "paid": true,
  "status": ""
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "status"

POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "A-2",
  "qty": 1,
  "paid": true,
  "status": "lost"
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "status"

# 26. A wrong type on a required field is a type_mismatch alone
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "A-2",
  "qty": "one",
  "paid": true,
  "status": "open"
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "type_mismatch"

# 27. An update that does not mention a required field succeeds
PUT http://127.0.0.1:{{port}}/data/orders/update/{{order_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "note": "gift wrap"
}
HTTP 200
[Asserts]
jsonpath "$.data.fields.note" == "gift wrap"
jsonpath "$.data.fields.ref" == "A-1"
jsonpath "$.diagnostics" not exists

# 28. An update that blanks a required field is rejected, and changes nothing
PUT http://127.0.0.1:{{port}}/data/orders/update/{{order_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": null
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "ref"

PUT http://127.0.0.1:{{port}}/data/orders/update/{{order_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": ""
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "required"

GET http://127.0.0.1:{{port}}/data/orders/get/{{order_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.fields.ref" == "A-1"
jsonpath "$.diagnostics" not exists

# 29. Making a field required later does not reject records written before:
# a read reports the gap as a warning (drift), never an error. The order from
# 21 has a note (27); this one does not.
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "A-3",
  "qty": 2,
  "paid": true,
  "status": "shipped"
}
HTTP 201
[Captures]
bare_id: jsonpath "$.data.id"

PUT http://127.0.0.1:{{port}}/schema/testuser/testapp/0-dev/field/orders/note
Authorization: Bearer {{token}}
Content-Type: application/json
{"required": true}
HTTP 200
[Asserts]
jsonpath "$.data.required" == true

# A field update that leaves required out keeps it
PUT http://127.0.0.1:{{port}}/schema/testuser/testapp/0-dev/field/orders/ref
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Ref"}
HTTP 200
[Asserts]
jsonpath "$.data.required" == true

# New creates must now give note
POST http://127.0.0.1:{{port}}/data/orders/add
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "ref": "A-4",
  "qty": 2,
  "paid": true,
  "status": "shipped"
}
HTTP 400
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "note"

GET http://127.0.0.1:{{port}}/data/orders/get/{{bare_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "warning"
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "note"

GET http://127.0.0.1:{{port}}/data/orders/list
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "warning"
jsonpath "$.diagnostics[0].path" == "{{bare_id}}/note"

# 30. /data/query reports it the same way — unless the query's fields leave
# note out, in which case note was not read and is not reported missing.
POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{ "queries": { "all": { "collection": "orders" } } }
HTTP 200
[Asserts]
jsonpath "$.data.all.records" count == 2
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].severity" == "warning"
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "all/{{bare_id}}/note"

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{ "queries": { "refs": { "collection": "orders", "fields": ["ref"] } } }
HTTP 200
[Asserts]
jsonpath "$.data.refs.records" count == 2
jsonpath "$.diagnostics" not exists

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{ "queries": { "notes": { "collection": "orders", "fields": ["ref", "note"] } } }
HTTP 200
[Asserts]
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].path" == "notes/{{bare_id}}/note"

# 31. An update that does not mention note still succeeds on that record
PUT http://127.0.0.1:{{port}}/data/orders/update/{{bare_id}}
X-Project-Id: testuser/testapp
X-Site-Id: dev
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "paid": false
}
HTTP 200
[Asserts]
jsonpath "$.data.fields.paid" == false
