# Discovery document, prefix catalogs, and a publish an agent can copy.
# dist.zip is index.html at the zip root (the file next to this suite).

GET http://127.0.0.1:{{port}}/.well-known/loco.json
HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
header "Cache-Control" == "no-cache"
jsonpath "$.name" == "loco"
jsonpath "$.version" isString
jsonpath "$.docs" count == 0
jsonpath "$.discovery" == "/.well-known/loco.json"
jsonpath "$.signup.handle" contains "1-63"
jsonpath "$.signup.handle" contains "a-z"
jsonpath "$.signup.rejected_handle" contains "400"
jsonpath "$.signup.rejected_handle" contains "1-63"
jsonpath "$.signup.missing_field" contains "400"
jsonpath "$.signup.missing_field" contains "password is required"
jsonpath "$.routes" count == 51
jsonpath "$.routes[0].method" == "GET"
jsonpath "$.routes[0].path" == "/.well-known/loco.json"

GET http://127.0.0.1:{{port}}/llms.txt
HTTP 200
[Asserts]
header "Content-Type" contains "text/plain"
body contains "/.well-known/loco.json"
body not contains "no such endpoint"

# A bare prefix names its routes. The error is not "no such endpoint".
GET http://127.0.0.1:{{port}}/auth
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.see" == "/.well-known/loco.json"
body contains "POST /auth/users"
body contains "POST /auth/login"
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "/schema/"
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "/data/"
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "/config/"
body not contains "no such endpoint"

# An unknown path under a prefix keeps the old error and gains see.
GET http://127.0.0.1:{{port}}/schema/foo
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"
jsonpath "$.see" == "/.well-known/loco.json"

GET http://127.0.0.1:{{port}}/help
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"
jsonpath "$.see" == "/.well-known/loco.json"

# GET /actions is the list, so a missing site is 400, not the prefix catalog.
GET http://127.0.0.1:{{port}}/actions
HTTP 400
[Asserts]
jsonpath "$.error" contains "missing project"

# Signup, as main answers it. A hyphen in the handle is 400 naming the
# account-name rule. A missing field is 400 JSON naming the field. An empty
# password is 400. A taken handle is 409. A wrong password at login is 401.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "lego-reseller", "name": "Lego Reseller", "password": "pass-word-1"}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" == "handle name \"lego-reseller\" must be 1-63 characters of a-z, 0-9, and _, starting with a letter or _"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "username"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "reseller", "name": "Reseller"}
HTTP 400
[Asserts]
jsonpath "$.error" == "password is required"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "reseller", "name": "Reseller", "password": "pass-word-1"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.username" == "reseller"
jsonpath "$.data.id" isString
jsonpath "$.data.token" not exists

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "reseller", "name": "Reseller", "password": "pass-word-1"}
HTTP 409
[Asserts]
jsonpath "$.error" == "user already exists"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "reseller", "password": "wrong-password"}
HTTP 401
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "invalid credentials"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "username"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "reseller", "password": "pass-word-1"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"
[Asserts]
jsonpath "$.data.user.username" == "reseller"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "inventory", "label": "Inventory", "description": "A parts list"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

GET http://127.0.0.1:{{port}}/config/site/reseller/inventory/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

POST http://127.0.0.1:{{port}}/schema/reseller/inventory/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "parts", "label": "Part", "label_plural": "Parts"}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "parts"

POST http://127.0.0.1:{{port}}/schema/reseller/inventory/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "parts", "name": "sku", "type": "string", "label": "SKU", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/reseller/inventory/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "parts", "name": "qty", "type": "integer", "label": "Quantity", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/reseller/inventory/0.0.1-dev/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "parts_public",
  "label": "Public parts",
  "collections": [
    {"collection": "parts", "read": true, "create": true, "update": true, "delete": false}
  ]
}
HTTP 201
[Asserts]
jsonpath "$.data.collections[0].read" == true
jsonpath "$.data.collections[0].delete" == false

PUT http://127.0.0.1:{{port}}/schema/reseller/inventory/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"public_permission_sets": ["parts_public"]}
HTTP 200
[Asserts]
jsonpath "$.data.public_permission_sets[0]" == "parts_public"

POST http://127.0.0.1:{{port}}/data/parts/add
Authorization: Bearer {{token}}
X-Project-Id: reseller/inventory
X-Site-Id: dev
Content-Type: application/json
{"sku": "3001", "qty": 2}
HTTP 201
[Captures]
record: jsonpath "$.data.id"
[Asserts]
jsonpath "$.data.fields.sku" == "3001"
jsonpath "$.data.fields.qty" == 2

PUT http://127.0.0.1:{{port}}/data/parts/update/{{record}}
Authorization: Bearer {{token}}
X-Project-Id: reseller/inventory
X-Site-Id: dev
Content-Type: application/json
{"qty": 3}
HTTP 200
[Asserts]
jsonpath "$.data.fields.qty" == 3
jsonpath "$.data.fields.sku" == "3001"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: reseller/inventory
X-Site-Id: dev
Content-Type: application/json
{"queries": {"parts": {"collection": "parts", "limit": 50}}}
HTTP 200
[Asserts]
jsonpath "$.data.parts.records" count == 1
jsonpath "$.data.parts.records[0].fields.sku" == "3001"
jsonpath "$.data.parts.cursor" == null

PUT http://127.0.0.1:{{port}}/schema/reseller/inventory/0.0.1-dev/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
[Asserts]
jsonpath "$.data.files" == 1
jsonpath "$.data.hash" isString

GET http://127.0.0.1:{{port}}/
Host: dev.inventory.reseller.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" contains "text/html"
body contains "discovery page"

GET http://127.0.0.1:{{port}}/.well-known/loco.json
Host: dev.inventory.reseller.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.name" == "loco"

GET http://127.0.0.1:{{port}}/llms.txt
Host: dev.inventory.reseller.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" contains "text/plain"
body contains "/.well-known/loco.json"

GET http://127.0.0.1:{{port}}/auth
Host: dev.inventory.reseller.localhost:{{port}}
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "POST /auth/users"
body not contains "no such endpoint"
body not contains "discovery page"

GET http://127.0.0.1:{{port}}/schema/foo
Host: dev.inventory.reseller.localhost:{{port}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"
jsonpath "$.see" == "/.well-known/loco.json"

GET http://127.0.0.1:{{port}}/data/parts/list
Host: dev.inventory.reseller.localhost:{{port}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].fields.sku" == "3001"

POST http://127.0.0.1:{{port}}/config/version/reseller/inventory
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.version" == "0.0.1"

POST http://127.0.0.1:{{port}}/config/site/reseller/inventory
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "Public", "version": "0.0.1", "dataset": "dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/
Host: www.inventory.reseller.localhost:{{port}}
HTTP 200
[Asserts]
body contains "discovery page"

GET http://127.0.0.1:{{port}}/data/parts/list
Host: www.inventory.reseller.localhost:{{port}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].fields.sku" == "3001"

# Mount check. discovery.rs asserts each of these request lines exists.
# A body that says "no such endpoint" means the path is not mounted.
# The discovery document itself contains that phrase, so its probe is 200 only.
POST http://127.0.0.1:{{port}}/auth/login
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/auth/logout
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/auth/me
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/auth/users
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/auth/users/x
HTTP *
[Asserts]
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/auth/users/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/auth/api-keys
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/auth/api-keys/list
HTTP *
[Asserts]
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/auth/api-keys/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/project
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/project/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/project/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/dataset/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/dataset/x/x/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/dataset/x/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/site/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/site/x/x/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/site/x/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/config/site/x/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/version/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/version/x/x/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/manifest
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/manifest
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/schema/x/x/x/collection
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/collection/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/collection/x
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/collection/x
HTTP *
[Asserts]
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/schema/x/x/x/collection/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/schema/x/x/x/field
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/field/x/list
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/field/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/schema/x/x/x/field/x/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/schema/x/x/x/permission_set
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/permission_set/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/permission_set/x
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/permission_set/x
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/bundle
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/bundle
HTTP *
[Asserts]
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/schema/x/x/x/bundle
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/data/x/add
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data/x/list
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data/x/get/x
HTTP *
[Asserts]
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/data/x/update/x
HTTP *
[Asserts]
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/data/x/delete/x
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data/x/fields
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/data/query
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/actions
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/actions/x
HTTP *
[Asserts]
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/actions/x
HTTP *
[Asserts]
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/.well-known/loco.json
HTTP 200

GET http://127.0.0.1:{{port}}/llms.txt
HTTP 200
[Asserts]
body not contains "no such endpoint"
