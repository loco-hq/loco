# /data through CollectionSource (issue #125).
#
# alice/pkg declares type warehouse. The runner registers a source for that
# type against three fixture upstreams. alice/shop installs east, west, and
# dead. alice/sync installs hub and adds a custom collection. alice/reader
# depends on sync and not on pkg, and still filters hub by the type's field.
#
# The dev site pins 0.0.1-dev, which project create bootstraps.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "pkg", "label": "Package", "description": "Owns warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "warehouse",
  "label": "Warehouse",
  "secrets": [
    {"name": "token", "label": "Token", "required": true}
  ],
  "variables": [
    {"name": "base_url", "label": "Base URL", "required": true}
  ],
  "collections": [
    {
      "name": "items",
      "label": "Item",
      "label_plural": "Items",
      "fields": [
        {"name": "status", "type": "string", "label": "Status"},
        {"name": "seen", "type": "string", "label": "Seen"}
      ]
    },
    {
      "name": "fail",
      "label": "Fail",
      "label_plural": "Fails",
      "fields": [
        {"name": "status", "type": "string", "label": "Status"}
      ]
    }
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Installs pkg"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/pkg@0.0.1-dev"]}
HTTP 200

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "east", "label": "East", "type": "alice/pkg.warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "west", "label": "West", "type": "alice/pkg.warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "dead", "label": "Dead", "type": "alice/pkg.warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "sync", "label": "Sync", "description": "Re-exports hub"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/sync/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/pkg@0.0.1-dev"]}
HTTP 200

POST http://127.0.0.1:{{port}}/schema/alice/sync/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "hub",
  "label": "Hub",
  "type": "alice/pkg.warehouse",
  "collections": [
    {
      "name": "note",
      "label": "Note",
      "label_plural": "Notes",
      "fields": [
        {"name": "body", "type": "string", "label": "Body"},
        {"name": "seen", "type": "string", "label": "Seen"}
      ]
    }
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "reader", "label": "Reader", "description": "Depends on sync only"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/reader/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/sync@0.0.1-dev"]}
HTTP 200

# ---------------------------------------------------------------------------
# Before any connection value is stored. Capability is checked before the
# required-value check, and that check is before the source is called.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/data/east:items/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.diagnostics" count == 2
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "token"
jsonpath "$.diagnostics[0].message" == "secret 'token' is not set"
jsonpath "$.diagnostics[1].kind" == "required"
jsonpath "$.diagnostics[1].path" == "base_url"
jsonpath "$.diagnostics[1].message" == "variable 'base_url' is not set"

POST http://127.0.0.1:{{port}}/data/east:items/add
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 400
[Asserts]
jsonpath "$.error" == "unsupported"
jsonpath "$.diagnostics[0].kind" == "unsupported"
jsonpath "$.diagnostics[0].message" contains "does not support insert"
body not contains "is not set"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "wide": {
      "collection": "east:items",
      "where": {"field": "alice/pkg.status", "op": "gt", "value": "m"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.wide.error" == "query failed"
jsonpath "$.data.wide.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.wide.diagnostics[0].message" contains "op 'gt'"
body not contains "is not set"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "stamped": {
      "collection": "east:items",
      "order": [{"field": "$created_at"}]
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.stamped.error" == "query failed"
jsonpath "$.data.stamped.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.stamped.diagnostics[0].message" contains "ordering by $created_at"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "open": {"collection": "east:items"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.open.error" == "query failed"
jsonpath "$.data.open.diagnostics" count == 2
jsonpath "$.data.open.diagnostics[0].kind" == "required"
jsonpath "$.data.open.diagnostics[0].path" == "token"
jsonpath "$.data.open.diagnostics[1].kind" == "required"
jsonpath "$.data.open.diagnostics[1].path" == "base_url"

# ---------------------------------------------------------------------------
# Each integration's values. dead points nowhere. reader stores hub's values
# on its own dataset, qualified, without depending on pkg.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/east:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "east-token"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/east:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{east}}"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/west:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "west-token"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/west:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{west}}"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/dead:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "dead-token"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/dead:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "http://127.0.0.1:9"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/reader/dev/alice%2Fsync.hub:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "hub-token"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/variable/alice/reader/dev/alice%2Fsync.hub:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{hub}}"}
HTTP 200

GET http://127.0.0.1:{{port}}/data/east:items/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].id" == "e1"
jsonpath "$.data[0].fields.status" == "open"
jsonpath "$.data[0].fields.seen" == "east-token"
jsonpath "$.data[0].created_at" not exists
jsonpath "$.data[0].dataset_id" not exists
jsonpath "$.data[0].owner" not exists
jsonpath "$.data[0].created_by" not exists
jsonpath "$.data[0].updated_at" not exists
jsonpath "$.data[0].updated_by" not exists
jsonpath "$.data[1].id" == "e2"
jsonpath "$.data[1].fields.status" == "closed"
jsonpath "$.data[1].fields.seen" == "east-token"

GET http://127.0.0.1:{{port}}/data/east:items/get/e1
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.id" == "e1"
jsonpath "$.data.fields.status" == "open"
jsonpath "$.data.fields.seen" == "east-token"
jsonpath "$.data.created_at" not exists
jsonpath "$.data.dataset_id" not exists

GET http://127.0.0.1:{{port}}/data/east:items/get/missing
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "record not found"

GET http://127.0.0.1:{{port}}/data/west:items/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].id" == "w1"
jsonpath "$.data[0].fields.status" == "open"
jsonpath "$.data[0].fields.seen" == "west-token"
jsonpath "$.data[0].created_at" not exists

DELETE http://127.0.0.1:{{port}}/data/east:items/delete/e1
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 400
[Asserts]
jsonpath "$.error" == "unsupported"
jsonpath "$.diagnostics[0].message" contains "does not support delete"
body not contains "east-token"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "still": {
      "collection": "east:items",
      "where": {"field": "alice/pkg.status", "op": "gt", "value": "m"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.still.diagnostics[0].kind" == "unsupported"
body not contains "east-token"

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "local", "label": "Local", "label_plural": "Locals"}
HTTP 201

POST http://127.0.0.1:{{port}}/data/local/add
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 201
[Asserts]
jsonpath "$.data.created_at" exists
jsonpath "$.data.dataset_id" == "alice/shop/dev"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "east": {
      "collection": "east:items",
      "where": {"field": "alice/pkg.status", "op": "eq", "value": "open"}
    },
    "west": {"collection": "west:items"},
    "local": {"collection": "local"},
    "down": {"collection": "east:fail"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.east.records" count == 1
jsonpath "$.data.east.records[0].id" == "e1"
jsonpath "$.data.east.records[0].fields.seen" == "east-token"
jsonpath "$.data.east.records[0].fields.status" == "open"
jsonpath "$.data.east.records[0].created_at" not exists
jsonpath "$.data.east.records[0].dataset_id" not exists
jsonpath "$.data.east.cursor" == null
jsonpath "$.data.west.records" count == 1
jsonpath "$.data.west.records[0].id" == "w1"
jsonpath "$.data.west.records[0].fields.seen" == "west-token"
jsonpath "$.data.west.records[0].created_at" not exists
jsonpath "$.data.local.records" count == 1
jsonpath "$.data.local.records[0].created_at" exists
jsonpath "$.data.local.records[0].dataset_id" == "alice/shop/dev"
jsonpath "$.data.down.error" == "query failed"
jsonpath "$.data.down.diagnostics[0].kind" == "upstream"
jsonpath "$.data.down.diagnostics[0].message" contains "upstream 503: upstream is down"
body not contains "127.0.0.1"

GET http://127.0.0.1:{{port}}/data/east:fail/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 502
[Asserts]
jsonpath "$.error" == "upstream 503: upstream is down"
body not contains "127.0.0.1"

GET http://127.0.0.1:{{port}}/data/dead:items/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 502
[Asserts]
jsonpath "$.error" startsWith "upstream 0:"
body not contains "127.0.0.1"

# reader does not depend on pkg. The address brings the type's fields in.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/reader
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "hub": {
      "collection": "alice/sync.hub:items",
      "where": {"field": "alice/pkg.status", "op": "eq", "value": "open"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.hub.records" count == 1
jsonpath "$.data.hub.records[0].id" == "h1"
jsonpath "$.data.hub.records[0].fields.status" == "open"
jsonpath "$.data.hub.records[0].fields.seen" == "hub-token"
jsonpath "$.data.hub.records[0].created_at" not exists
jsonpath "$.data.hub.cursor" == null

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/reader
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "bare": {
      "collection": "alice/sync.hub:items",
      "where": {"field": "status", "op": "eq", "value": "open"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.bare.error" == "query failed"
jsonpath "$.data.bare.diagnostics[0].kind" == "unknown_field"
jsonpath "$.data.bare.diagnostics[0].message" contains "write alice/pkg.status"
body not contains "not a direct dependency"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/reader
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "plain": {"collection": "alice/pkg.items"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.plain.error" == "query failed"
jsonpath "$.data.plain.diagnostics[0].kind" == "unknown_collection"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/reader
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "notes": {
      "collection": "alice/sync.hub:note",
      "where": {"field": "alice/sync.body", "op": "eq", "value": "hi"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.notes.records" count == 1
jsonpath "$.data.notes.records[0].id" == "n1"
jsonpath "$.data.notes.records[0].fields.body" == "hi"
jsonpath "$.data.notes.records[0].fields.seen" == "hub-token"
jsonpath "$.data.notes.records[0].created_at" not exists
