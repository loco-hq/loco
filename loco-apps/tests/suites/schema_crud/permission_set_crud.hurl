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
  "read": ["notes"],
  "create": ["notes"]
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "public_notes"
jsonpath "$.data.read[0]" == "notes"
jsonpath "$.data.create[0]" == "notes"

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
  "create": []
}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Anonymous notes"
jsonpath "$.data.create" count == 0
jsonpath "$.data.read[0]" == "notes"

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/public_notes
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/permission_set/public_notes
Authorization: Bearer {{token}}
HTTP 404
