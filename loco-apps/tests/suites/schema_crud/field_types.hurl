# A field may declare only string, integer, float, or boolean — on create and
# on update. Anything else is a 400 and nothing is stored.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "typed"}
HTTP 201

# 1. Each scalar is accepted.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "s", "type": "string"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "i", "type": "integer"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "f", "type": "float"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "b", "type": "boolean"}
HTTP 201

# 2. list is not a field type: the lake cannot store one.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "tags", "type": "list"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown field type 'list'"

# 3. Nor is anything else.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "when", "type": "datetime"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "unknown field type 'datetime'"

# 4. A missing type is not a type either.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "typed", "name": "untyped"}
HTTP 400

# 5. The rejected fields were not stored, nor added to the default fieldset.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field/typed/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 4

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/fieldset/typed/default
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.fields" count == 4

# 6. Update checks the type too, and leaves the field as it was.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field/typed/s
Authorization: Bearer {{token}}
Content-Type: application/json
{"type": "list"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "unknown field type 'list'"

# 7. An update to another scalar, or one that leaves the type out, is fine.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field/typed/s
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "S"}
HTTP 200
[Asserts]
jsonpath "$.data.type" == "string"
jsonpath "$.data.label" == "S"

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/field/typed/i
Authorization: Bearer {{token}}
Content-Type: application/json
{"type": "float"}
HTTP 200
[Asserts]
jsonpath "$.data.type" == "float"

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-dev/collection/typed
Authorization: Bearer {{token}}
HTTP 200
