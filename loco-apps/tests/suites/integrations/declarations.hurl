# Integration types and integrations on a version (issue #123).
#
# A draft declares a type with inline secrets, variables, a standard
# collection (and its fields), and a type action. A second type reuses the
# same collection and action names on its own document. An integration of
# the first type stores a qualified self reference bare and carries a custom
# collection inline. A published version refuses a write of either document.
# Version copy carries the lists. A second project declares its own
# integration of that type. A custom collection that reuses a name the type
# offers is a 400 naming both. Writing the dependency's type or integration
# by its qualified name does not change that document. Deleting the
# integration removes the custom collections that were on it. Deleting the
# draft version removes the type.
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
# Draft declares a type. Secrets have no default or value. Collections and
# actions are lists on the same document. A body that puts default or value
# on a secret is a 400 and stores nothing.
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
  ],
  "collections": [
    {
      "name": "orders",
      "label": "Orders",
      "description": "BrickLink orders",
      "fields": [
        {"name": "status", "type": "string", "label": "Status", "required": true, "options": [{"value": "paid", "label": "Paid"}]}
      ]
    }
  ],
  "actions": [
    {
      "name": "set_status",
      "label": "Set status",
      "description": "Sets an order status",
      "params": [
        {"name": "order_id", "type": "string", "label": "Order", "required": true},
        {"name": "status", "type": "string", "label": "Status", "required": true, "options": [{"value": "paid", "label": "Paid"}]}
      ]
    }
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
jsonpath "$.data.collections" count == 1
jsonpath "$.data.collections[0].name" == "orders"
jsonpath "$.data.collections[0].fields[0].name" == "status"
jsonpath "$.data.collections[0].fields[0].type" == "string"
jsonpath "$.data.collections[0].fields[0].options[0].value" == "paid"
jsonpath "$.data.actions" count == 1
jsonpath "$.data.actions[0].name" == "set_status"
jsonpath "$.data.actions[0].params" count == 2
jsonpath "$.data.actions[0].params[0].name" == "order_id"
jsonpath "$.data.actions[0].params[1].options[0].value" == "paid"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "BrickLink"
jsonpath "$.data.secrets[0].name" == "consumer_key"
jsonpath "$.data.collections[0].fields[0].name" == "status"

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

# A label-only PUT leaves the lists.
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
jsonpath "$.data.collections" count == 1
jsonpath "$.data.collections[0].name" == "orders"
jsonpath "$.data.actions[0].name" == "set_status"

# Naming secrets replaces that list and leaves the others.
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
jsonpath "$.data.collections[0].name" == "orders"

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
jsonpath "$.data.collections[0].label" == "Orders"

# A repeated collection name is a 400 and the stored list stays.
PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [
  {"name": "orders", "label": "One"},
  {"name": "orders", "label": "Two"}
]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "orders"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections" count == 1
jsonpath "$.data.collections[0].label" == "Orders"

# Options on a non-string field are a 400 and the stored list stays.
PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [
  {"name": "orders", "label": "Orders", "fields": [
    {"name": "qty", "type": "integer", "label": "Qty", "options": [{"value": "1", "label": "One"}]}
  ]}
]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "qty"
jsonpath "$.error" contains "string"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections[0].fields[0].name" == "status"

# Naming collections replaces the list.
PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [
  {"name": "orders", "label": "Orders", "fields": [
    {"name": "status", "type": "string", "label": "Status", "required": true}
  ]},
  {"name": "order_items", "label": "Order items", "fields": [
    {"name": "item_no", "type": "string", "label": "Item"}
  ]}
]}
HTTP 200
[Asserts]
jsonpath "$.data.collections" count == 2
jsonpath "$.data.collections[1].name" == "order_items"
jsonpath "$.data.actions[0].name" == "set_status"
jsonpath "$.data.secrets" count == 2

# ---------------------------------------------------------------------------
# A second type reuses the collection and action names. An ordinary
# collection and action of those names stay in the ordinary stores.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "warehouse",
  "label": "Warehouse",
  "collections": [{"name": "orders", "label": "Warehouse orders"}],
  "actions": [{"name": "set_status", "label": "Warehouse status"}]
}
HTTP 201
[Asserts]
jsonpath "$.data.collections[0].label" == "Warehouse orders"
jsonpath "$.data.actions[0].label" == "Warehouse status"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections[0].label" == "Orders"
jsonpath "$.data.actions[0].label" == "Set status"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type/warehouse
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections[0].label" == "Warehouse orders"
jsonpath "$.data.actions[0].label" == "Warehouse status"

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

# A qualified self reference is stored bare. A custom collection the type
# already offers is a 400 naming both. A different name is stored inline.
POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "store",
  "label": "Store",
  "type": "alice/pkg.bricklink",
  "collections": [{"name": "orders", "label": "Stolen"}]
}
HTTP 400
[Asserts]
jsonpath "$.error" contains "orders"
jsonpath "$.error" contains "bricklink"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration/store
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "store",
  "label": "Store",
  "type": "alice/pkg.bricklink",
  "collections": [
    {
      "name": "invoice",
      "label": "Invoice",
      "fields": [{"name": "amount", "type": "string", "label": "Amount"}]
    }
  ]
}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "store"
jsonpath "$.data.type" == "bricklink"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.collections[0].name" == "invoice"
jsonpath "$.data.collections[0].fields[0].name" == "amount"

# A label-only PUT leaves the custom collections. Naming collections replaces them.
PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration/store
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Main store"}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Main store"
jsonpath "$.data.collections[0].name" == "invoice"
jsonpath "$.data.type" == "bricklink"

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration/store
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [{"name": "orders", "label": "Stolen"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "orders"
jsonpath "$.error" contains "bricklink"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration/store
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections" count == 1
jsonpath "$.data.collections[0].name" == "invoice"

# ---------------------------------------------------------------------------
# Copy carries the type document (inline collections, fields, and actions)
# and the integration document. The published version then refuses writes.
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
jsonpath "$.data.collections" count == 2
jsonpath "$.data.collections[0].label" == "Orders"
jsonpath "$.data.collections[0].fields[0].name" == "status"
jsonpath "$.data.actions[0].params" count == 2

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.type" == "bricklink"
jsonpath "$.data.version" == "1.0.0"
jsonpath "$.data.collections[0].name" == "invoice"
jsonpath "$.data.collections[0].fields[0].name" == "amount"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 4
jsonpath "$.data[0].name" == "orders"
jsonpath "$.data[0].integration" not exists
jsonpath "$.data[1].integration" == "store"
jsonpath "$.data[1].name" == "orders"
jsonpath "$.data[2].integration" == "store"
jsonpath "$.data[2].name" == "order_items"
jsonpath "$.data[3].integration" == "store"
jsonpath "$.data[3].name" == "invoice"

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

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [{"name": "extra", "label": "Extra"}]}
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

PUT http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [{"name": "extra", "label": "Extra"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

# ---------------------------------------------------------------------------
# The installer depends on the published package and declares its own
# integration. A collection the type offers is refused. Writing the
# dependency's documents by their qualified names does not change them.
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
jsonpath "$.data.collections" count == 2
jsonpath "$.data.collections[0].name" == "orders"
jsonpath "$.data.actions[0].name" == "set_status"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "sf_east",
  "label": "East",
  "type": "alice/pkg.bricklink",
  "collections": [{"name": "orders", "label": "Stolen"}]
}
HTTP 400
[Asserts]
jsonpath "$.error" contains "orders"
jsonpath "$.error" contains "alice/pkg.bricklink"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "sf_east",
  "label": "East",
  "type": "alice/pkg.bricklink",
  "collections": [
    {
      "name": "invoice",
      "label": "Invoice",
      "fields": [{"name": "amount", "type": "string", "label": "Amount"}]
    }
  ]
}
HTTP 201
[Asserts]
jsonpath "$.data.type" == "alice/pkg.bricklink"
jsonpath "$.data.project" == "alice/shop"
jsonpath "$.data.collections[0].name" == "invoice"
jsonpath "$.data.collections[0].fields[0].name" == "amount"

# The path name is this version's document. A qualified name is not one of
# the shop's documents, so the write is not found and the package is unchanged.
PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/alice%2Fpkg.store
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [{"name": "invoice", "label": "Stolen"}]}
HTTP 404
[Asserts]
jsonpath "$.error" contains "not found"
jsonpath "$.error" contains "alice/pkg.store"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections" count == 1
jsonpath "$.data.collections[0].name" == "invoice"

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration_type/alice%2Fpkg.bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [{"name": "extra", "label": "Extra"}]}
HTTP 404
[Asserts]
jsonpath "$.error" contains "not found"
jsonpath "$.error" contains "alice/pkg.bricklink"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration_type/bricklink
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections" count == 2
jsonpath "$.data.collections[0].name" == "orders"

# A read of the dependency's integration still resolves.
GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/alice%2Fpkg.store
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "store"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.collections[0].name" == "invoice"

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
jsonpath "$.data.collections[0].label" == "Invoice"
jsonpath "$.data.collections[0].fields[0].name" == "amount"

PUT http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east
Authorization: Bearer {{token}}
Content-Type: application/json
{"collections": [{"name": "other", "label": "Other"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

# Deleting the draft integration removes the custom collections that were on
# it. The published copy keeps them.
DELETE http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration/sf_east
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/shop/1.0.0/integration/sf_east
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections[0].name" == "invoice"
jsonpath "$.data.collections[0].fields[0].name" == "amount"

# ---------------------------------------------------------------------------
# Version delete removes the package draft. The dev site moves to the copy
# first. The copy still has the type, its inline collections, and the store.
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
jsonpath "$.data.collections[0].name" == "orders"
jsonpath "$.data.actions[0].name" == "set_status"

GET http://127.0.0.1:{{port}}/schema/alice/pkg/1.0.0/integration/store
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections[0].fields[0].name" == "amount"
