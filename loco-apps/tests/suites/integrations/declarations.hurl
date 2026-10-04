# Integration types and integrations on a version (issue #123).
#
# A draft declares a type (inline secrets and variables, a standard collection
# with a field, and a type action), and a second type that reuses the same
# collection and action names. It declares an integration of the first type.
# A published version refuses each write. Version copy carries them.
# A second project declares an integration of that type and a custom
# collection on it, and is refused when it tries to declare a custom
# collection on the dependency's integration or a standard collection on the
# dependency's type. Deleting the integration removes its custom collection
# and field. Deleting the draft version removes the type.
#
# No fixtures. alice creates alice/pkg (the package) and alice/shop (the
# installer). Each bootstraps 0.0.1-dev, a dev dataset, and a dev site.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "pkg", "label": "Package", "description": "Integration type"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Installer"}
HTTP 201

# ---------------------------------------------------------------------------
# Draft declares a type. Secrets have no default or value. A variable
# round-trips its default. A body that puts either on a secret is a 400.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "bricklink",
  "label": "BrickLink",
  "description": "BrickLink store API",
  "secrets": [
    {"name": "consumer_key", "label": "Consumer key", "description": "OAuth consumer key", "required": true}
  ],
  "variables": [
    {"name": "base_url", "label": "API base", "required": false, "default": "https://api.bricklink.com/api/store/v1"}
  ]
}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "bricklink"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.secrets" count == 1
jsonpath "$.data.secrets[0].name" == "consumer_key"
jsonpath "$.data.secrets[0].required" == true
jsonpath "$.data.secrets[0].default" not exists
jsonpath "$.data.secrets[0].value" not exists
jsonpath "$.data.variables[0].name" == "base_url"
jsonpath "$.data.variables[0].default" == "https://api.bricklink.com/api/store/v1"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "BrickLink"
jsonpath "$.data.secrets[0].name" == "consumer_key"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "leaky", "label": "Leaky", "secrets": [{"name": "token", "label": "Token", "value": "secret"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "value"
jsonpath "$.error" contains "token"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/leaky
Authorization: Bearer {{token}}
HTTP 404

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "BrickLink store"}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "BrickLink store"
jsonpath "$.data.description" == "BrickLink store API"
jsonpath "$.data.secrets" count == 1
jsonpath "$.data.secrets[0].name" == "consumer_key"
jsonpath "$.data.variables[0].default" == "https://api.bricklink.com/api/store/v1"

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"secrets": [
  {"name": "consumer_key", "label": "Consumer key", "required": true},
  {"name": "token", "label": "Token", "required": true}
]}
HTTP 200
[Asserts]
jsonpath "$.data.secrets" count == 2
jsonpath "$.data.secrets[1].name" == "token"
jsonpath "$.data.variables" count == 1
jsonpath "$.data.variables[0].name" == "base_url"

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"variables": [{"name": "consumer_key", "label": "Nope"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "share the name"
jsonpath "$.error" contains "consumer_key"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.secrets" count == 2
jsonpath "$.data.variables[0].name" == "base_url"

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"secrets": [{"name": "consumer_key", "label": "Consumer key", "default": "nope"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "default"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.secrets" count == 2
jsonpath "$.data.secrets[1].name" == "token"

# ---------------------------------------------------------------------------
# Standard collection, field, and type action. A second type reuses both
# names. An ordinary collection and action of those names stay separate.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "orders", "label": "Orders", "label_plural": "Orders"}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "orders"
jsonpath "$.data.integration_type" == "bricklink"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.version" == "0.0.1-dev"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "orders", "name": "status", "type": "string", "label": "Status", "required": true}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "status"
jsonpath "$.data.collection" == "orders"
jsonpath "$.data.type" == "string"
jsonpath "$.data.integration_type" == "bricklink"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/field/orders/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "status"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/action
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "set_status",
  "label": "Set status",
  "description": "Sets an order status",
  "params": [
    {"name": "order_id", "type": "string", "label": "Order", "required": true},
    {"name": "status", "type": "string", "label": "Status", "required": true, "options": [{"value": "paid", "label": "Paid"}]}
  ]
}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "set_status"
jsonpath "$.data.integration_type" == "bricklink"
jsonpath "$.data.params" count == 2
jsonpath "$.data.params[0].name" == "order_id"
jsonpath "$.data.params[1].options[0].value" == "paid"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "warehouse", "label": "Warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/warehouse/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "orders", "label": "Warehouse orders"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/warehouse/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "set_status", "label": "Warehouse status"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/collection/orders
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Orders"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/warehouse/collection/orders
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Warehouse orders"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/action/set_status
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Set status"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/warehouse/action/set_status
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Warehouse status"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "orders", "label": "Ordinary orders"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "set_status", "label": "Ordinary status"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].label" == "Ordinary orders"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].label" == "Ordinary status"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# A qualified self reference is stored bare.
POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "store", "label": "Store", "type": "alice/pkg.bricklink"}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "store"
jsonpath "$.data.type" == "bricklink"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.version" == "0.0.1-dev"

# ---------------------------------------------------------------------------
# Copy carries the type, its collection, field, and action, and the
# integration. The published version then refuses each write.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/config/version/alice/pkg
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "1.0.0"
jsonpath "$.data.secrets" count == 2
jsonpath "$.data.variables[0].default" == "https://api.bricklink.com/api/store/v1"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink/collection/orders
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Orders"
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink/field/orders/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "status"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink/action/set_status
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.params" count == 2

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.type" == "bricklink"
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "extra", "label": "Extra"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "orders", "name": "extra", "type": "string", "label": "Extra"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "extra", "label": "Extra"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other", "type": "bricklink"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "extra", "label": "Extra"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "extra", "name": "amount", "type": "string", "label": "Amount"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

# ---------------------------------------------------------------------------
# The installer depends on the published package, declares its own
# integration and a custom collection, and is refused on the package's.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/pkg@1.0.0"]}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration_type/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[*].project" includes "alice/pkg"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration_type/alice%2Fpkg.bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "bricklink"
jsonpath "$.data.project" == "alice/pkg"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration_type/alice%2Fpkg.bricklink/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "orders"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "sf_east", "label": "East", "type": "alice/pkg.bricklink"}
HTTP 201
[Asserts]
jsonpath "$.data.type" == "alice/pkg.bricklink"
jsonpath "$.data.project" == "alice/shop"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "invoice", "label": "Invoice", "label_plural": "Invoices"}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "invoice"
jsonpath "$.data.integration" == "sf_east"
jsonpath "$.data.project" == "alice/shop"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "invoice", "name": "amount", "type": "string", "label": "Amount"}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "amount"
jsonpath "$.data.integration" == "sf_east"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/alice%2Fpkg.store/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "invoice", "label": "Stolen"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "custom collection"
jsonpath "$.error" contains "alice/pkg.store"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/alice%2Fpkg.store/collection/invoice
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/alice%2Fpkg.store/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/alice%2Fpkg.store/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "invoice", "name": "amount", "type": "string", "label": "Amount"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "custom collection"
jsonpath "$.error" contains "alice/pkg.store"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration_type/alice%2Fpkg.bricklink/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "extra", "label": "Extra"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "type's owner"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration_type/alice%2Fpkg.bricklink/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

POST http://127.0.0.1:{{port}}/config/version/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0", "from": "0.0.1-dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.type" == "alice/pkg.bricklink"
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east/collection/invoice
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Invoice"

GET http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east/field/invoice/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "amount"

POST http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

# Deleting the draft integration removes its custom collection and field.
# The published copy keeps them.
DELETE http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east/collection/invoice
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east/field/invoice/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east/collection/invoice
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east/field/invoice/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# ---------------------------------------------------------------------------
# Version delete removes the package draft. The dev site moves to the copy
# first. The copy still has the type.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/site/alice/pkg/dev
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0"}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/version/alice/pkg/0.0.1-dev
Authorization: Bearer {{token}}
HTTP 200

POST http://127.0.0.1:{{port}}/config/version/alice/pkg
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1-dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.secrets[0].name" == "consumer_key"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store
Authorization: Bearer {{token}}
HTTP 200
