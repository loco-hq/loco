# Permission set CRUD on /schema

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "public_notes",
  "label": "Public notes",
  "collections": [
    { "collection": "notes", "read": true, "create": true }
  ]
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "public_notes"
jsonpath "$.data.collections[0].collection" == "notes"
jsonpath "$.data.collections[0].read" == true
jsonpath "$.data.collections[0].create" == true
jsonpath "$.data.collections[0].update" == false
jsonpath "$.data.collections[0].delete" == false

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "public_notes"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/public_notes
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Public notes"

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/public_notes
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "label": "Anonymous notes",
  "collections": [
    { "collection": "notes", "read": true }
  ]
}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Anonymous notes"
jsonpath "$.data.collections[0].read" == true
jsonpath "$.data.collections[0].create" == false

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/public_notes
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/public_notes
Authorization: Bearer {{token}}
HTTP 404
