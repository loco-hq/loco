# Refuse deleting an integration type an integration still names (issue #130).
#
# An unused type deletes. A type an integration in the same version names is
# 409 naming that integration, and the type stays until the integration is
# gone. A type a dependent project's integration names is 409 naming that
# project's integration. No fixtures. alice creates alice/catalog (the
# package) and alice/buyer (the installer). Each bootstraps 0.0.1-dev.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "catalog", "label": "Catalog", "description": "Integration type"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "buyer", "label": "Buyer", "description": "Installer"}
HTTP 201

# An unused type deletes.
POST http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "unused", "label": "Unused"}
HTTP 201

DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/unused
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/unused
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "bricklink", "label": "BrickLink"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "outlet", "label": "Outlet", "type": "bricklink"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "store", "label": "Store", "type": "bricklink"}
HTTP 201

# Both integrations in this version are named, and the type stays.
DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "alice/catalog.bricklink"
jsonpath "$.error" contains "alice/catalog@0.0.1-dev outlet"
jsonpath "$.error" contains "alice/catalog@0.0.1-dev store"

GET http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "BrickLink"

DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration/outlet
Authorization: Bearer {{token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration/store
Authorization: Bearer {{token}}
HTTP 200

# With nothing left in this version naming it, the type deletes.
DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 404

# The installer depends on this draft and declares two integrations of the
# type. Deleting the type names both, and the type stays.
POST http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "bricklink", "label": "BrickLink"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/buyer/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/catalog@0.0.1-dev"]}
HTTP 200

POST http://127.0.0.1:{{port}}/schema/alice/buyer/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "east", "label": "East", "type": "alice/catalog.bricklink"}
HTTP 201
[Asserts]
jsonpath "$.data.type" == "alice/catalog.bricklink"

POST http://127.0.0.1:{{port}}/schema/alice/buyer/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "west", "label": "West", "type": "alice/catalog.bricklink"}
HTTP 201

DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "alice/buyer@0.0.1-dev east"
jsonpath "$.error" contains "alice/buyer@0.0.1-dev west"
jsonpath "$.error" not contains "alice/catalog@0.0.1-dev"

GET http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "bricklink"

DELETE http://127.0.0.1:{{port}}/schema/alice/buyer/0.0.1-dev/integration/east
Authorization: Bearer {{token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/schema/alice/buyer/0.0.1-dev/integration/west
Authorization: Bearer {{token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/catalog/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 404
