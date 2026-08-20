# Fieldset behavior: auto-create on collection create, auto-append on field
# create, cascade-remove on field delete, and explicit reorder via PUT.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Create a collection — should auto-create a "default" fieldset.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"name": "notes", "label": "Note"}
HTTP 201

# 2. Default fieldset exists, is auto_add, with empty fields list.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/fieldset/notes/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "default"
jsonpath "$.data[0].auto_add" == true
jsonpath "$.data[0].fields" count == 0

# 3. Create three fields. Each should append to the default fieldset in
#    the order it was created.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"collection": "notes", "name": "title", "type": "string"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"collection": "notes", "name": "body", "type": "string"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"collection": "notes", "name": "archived", "type": "boolean"}
HTTP 201

# 4. Default fieldset now lists fields in creation order.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/fieldset/notes/default
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.fields[0]" == "title"
jsonpath "$.data.fields[1]" == "body"
jsonpath "$.data.fields[2]" == "archived"

# 5. /field/list returns fields in fieldset order, not alphabetical.
#    (Alphabetical would be archived, body, title.)
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/notes/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].name" == "title"
jsonpath "$.data[1].name" == "body"
jsonpath "$.data[2].name" == "archived"

# 6. Reorder via PUT: move "archived" to the front.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/fieldset/notes/default
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"fields": ["archived", "title", "body"]}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/notes/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data[0].name" == "archived"
jsonpath "$.data[1].name" == "title"
jsonpath "$.data[2].name" == "body"

# 7. Cascade-delete: removing a field strips it from every fieldset.
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/notes/title
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/fieldset/notes/default
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.fields" count == 2
jsonpath "$.data.fields[0]" == "archived"
jsonpath "$.data.fields[1]" == "body"

# 8. Deleting the collection cascades to fieldsets too.
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/notes
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/fieldset/notes/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0
