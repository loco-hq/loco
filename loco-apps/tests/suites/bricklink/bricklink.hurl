# loco/bricklink (issue #104): live BrickLink orders through /data and /data/query.
#
# The loco seed account supplies loco/bricklink@1.0.0. alice/brocks depends on
# it (its manifest is a fixture on disk: /schema would refuse the dependency,
# because alice cannot read loco/bricklink until projects can be marked
# installable). The server is built with Extensions::default(), so the source
# is the production registration. {{bricklink}} is the runner's fixture server,
# which serves tests/fixtures/bricklink/ and checks each request's OAuth
# signature. Site dev holds good credentials; site other holds a consumer key
# BrickLink does not know.
#
# base_url is set before any secret, so no request ever reaches the default
# host (the real store API).

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# ---------------------------------------------------------------------------
# The package, as a store sees it.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:order_items/fields
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data[?(@.name == 'qty')].type" nth 0 == "integer"
jsonpath "$.data[?(@.name == 'color_id')].type" nth 0 == "integer"
jsonpath "$.data[?(@.name == 'order_id')].type" nth 0 == "string"

GET http://127.0.0.1:{{port}}/actions
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data[?(@.project == 'loco/bricklink')].name" includes "set_status"

# Declared, with no handler: read-only in 1.0.0.
POST http://127.0.0.1:{{port}}/actions/loco%2Fbricklink.store:set_status
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{"input": {"order_id": "29471234", "status": "PACKED"}}
HTTP 501
[Asserts]
jsonpath "$.error" == "no handler for action loco/bricklink.bricklink.set_status"

# ---------------------------------------------------------------------------
# Nothing stored yet. base_url has a default, so only the four secrets are
# missing, and no call is made.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/list
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 400
[Asserts]
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.diagnostics" count == 4
jsonpath "$.diagnostics[*].message" includes "secret 'consumer_key' is not set"
jsonpath "$.diagnostics[*].message" includes "secret 'token_secret' is not set"
body not contains "base_url"

# Read-only: insert is not a declared verb. Checked before configuration.
POST http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/add
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{"status": "PENDING"}
HTTP 400
[Asserts]
jsonpath "$.error" == "unsupported"
jsonpath "$.diagnostics[0].kind" == "unsupported"
jsonpath "$.diagnostics[0].message" contains "does not support insert"

# ---------------------------------------------------------------------------
# Values. base_url first on both datasets.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/variable/alice/brocks/dev/loco%2Fbricklink.store:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{bricklink}}"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/variable/alice/brocks/other/loco%2Fbricklink.store:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{bricklink}}"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/dev/loco%2Fbricklink.store:consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "fixture-consumer-key"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/dev/loco%2Fbricklink.store:consumer_secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "fixture-consumer-secret"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/dev/loco%2Fbricklink.store:token_value
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "fixture-token"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/dev/loco%2Fbricklink.store:token_secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "fixture-token-secret"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/other/loco%2Fbricklink.store:consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "unregistered-key-7f3a"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/other/loco%2Fbricklink.store:consumer_secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "other-consumer-secret"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/other/loco%2Fbricklink.store:token_value
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "other-token-9c1e"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/other/loco%2Fbricklink.store:token_secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "other-token-secret"}
HTTP 200

GET http://127.0.0.1:{{port}}/config/secret/alice/brocks/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[?(@.integration == 'loco/bricklink.store')]" count == 4
body not contains "fixture-consumer-secret"

# ---------------------------------------------------------------------------
# /data. The record id is the BrickLink order id. Only live fields come back.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/list
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 3
jsonpath "$.data[*].id" includes "29471234"
jsonpath "$.data[*].id" includes "29471236"
jsonpath "$.data[?(@.id == '29471234')].fields.status" nth 0 == "PENDING"
jsonpath "$.data[?(@.id == '29471234')].fields.buyer" nth 0 == "ada_bricks"
jsonpath "$.data[?(@.id == '29471234')].fields.date" nth 0 == "2026-10-03T14:05:11.000Z"
jsonpath "$.data[?(@.id == '29471234')].fields.totals" nth 0 == "USD 41.2300"
jsonpath "$.diagnostics" not exists
body not contains "created_at"
body not contains "buyer_email"

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store%3Aorders/get/29471234
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.id" == "29471234"
jsonpath "$.data.fields.status" == "PENDING"

# A filed order is not in `orders`, so get agrees with list.
GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/get/29471200
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "record not found"

# BrickLink's meta.code 404.
GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/get/11111111
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 404

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/get/not-an-order
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 404

# Item ids are {order_id}-{batch}-{inventory_id}: one lot can be in two
# batches of one order, and each is its own line.
GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:order_items/get/29471234-1-358001234
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.id" == "29471234-1-358001234"
jsonpath "$.data.fields.item_no" == "3001"
jsonpath "$.data.fields.item_type" == "PART"
jsonpath "$.data.fields.color_id" == 11
jsonpath "$.data.fields.color_name" == "Black"
jsonpath "$.data.fields.condition" == "N"
jsonpath "$.data.fields.qty" == 250
jsonpath "$.data.fields.remarks" == "A-12-3"
jsonpath "$.data.fields.inventory_id" == "358001234"
jsonpath "$.data.fields.order_id" == "29471234"
jsonpath "$.diagnostics" not exists

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:order_items/get/29471234-2-358001234
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.fields.inventory_id" == "358001234"
jsonpath "$.data.fields.qty" == 30

# A line with no inventory id is not dropped. Its id is its position.
GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:order_items/get/29471234-2-x3
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.fields.item_no" == "3062b"
jsonpath "$.data.fields.qty" == 8
jsonpath "$.data.fields.inventory_id" not exists

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:order_items/get/29471234-9-358001234
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 404

# There is no all-items endpoint. Listing every line is unsupported, the same
# 400 and kind as an undeclared verb, not a fan-out over every order.
GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:order_items/list
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 400
[Asserts]
jsonpath "$.error" == "unsupported"
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "unsupported"
jsonpath "$.diagnostics[0].message" contains "order_items needs an order_id filter"

# ---------------------------------------------------------------------------
# /data/query. Fields are the type owner's: loco/bricklink.status.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "pending": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "loco/bricklink.status", "op": "eq", "value": "PENDING"},
      "order": [{"field": "loco/bricklink.date", "dir": "desc"}]
    },
    "paid": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "loco/bricklink.status", "op": "in", "value": ["PAID", "SHIPPED"]},
      "fields": ["loco/bricklink.status"]
    },
    "one": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "$id", "op": "eq", "value": "29471236"}
    },
    "lines": {
      "collection": "loco/bricklink.store:order_items",
      "where": {"field": "loco/bricklink.order_id", "op": "in", "value": ["29471234", "29471235", "11111111"]},
      "order": [{"field": "loco/bricklink.qty", "dir": "desc"}]
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.pending.records" count == 2
jsonpath "$.data.pending.records[0].id" == "29471235"
jsonpath "$.data.pending.records[1].id" == "29471234"
jsonpath "$.data.pending.cursor" == null
jsonpath "$.data.paid.records" count == 1
jsonpath "$.data.paid.records[0].id" == "29471236"
jsonpath "$.data.paid.records[0].fields.status" == "PAID"
jsonpath "$.data.paid.records[0].fields.buyer" not exists
jsonpath "$.data.one.records" count == 1
jsonpath "$.data.one.records[0].fields.buyer" == "linus_lugs"
jsonpath "$.data.lines.records" count == 6
jsonpath "$.data.lines.records[0].fields.qty" == 250
jsonpath "$.data.lines.records[1].fields.qty" == 40
jsonpath "$.data.lines.records[1].fields.order_id" == "29471235"
jsonpath "$.data.lines.records[2].id" == "29471234-2-358001234"
jsonpath "$.data.lines.records[2].fields.qty" == 30
jsonpath "$.data.lines.records[4].id" == "29471234-2-x3"
jsonpath "$.data.lines.records[5].fields.qty" == 1
jsonpath "$.diagnostics" not exists

# Pages over one order's lines, with a cursor.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "lines": {
      "collection": "loco/bricklink.store:order_items",
      "where": {"field": "loco/bricklink.order_id", "op": "eq", "value": "29471234"},
      "limit": 3
    }
  }
}
HTTP 200
[Captures]
cursor: jsonpath "$.data.lines.cursor"
[Asserts]
jsonpath "$.data.lines.records" count == 3
jsonpath "$.data.lines.records[0].id" == "29471234-1-358001234"
jsonpath "$.data.lines.records[2].id" == "29471234-2-358001234"
jsonpath "$.data.lines.cursor" isString

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "lines": {
      "collection": "loco/bricklink.store:order_items",
      "where": {"field": "loco/bricklink.order_id", "op": "eq", "value": "29471234"},
      "limit": 3,
      "cursor": "{{cursor}}"
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.lines.records" count == 2
jsonpath "$.data.lines.records[0].id" == "29471234-2-358001299"
jsonpath "$.data.lines.records[1].id" == "29471234-2-x3"
jsonpath "$.data.lines.cursor" == null

# Shapes the source cannot honor. Each is that query's `unsupported` result,
# never a wider list: an op it does not declare, a system field it does not
# have, a filter field BrickLink cannot apply, and items without an order.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "after": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "loco/bricklink.date", "op": "gt", "value": "2026-10-01"}
    },
    "stamped": {
      "collection": "loco/bricklink.store:orders",
      "order": [{"field": "$created_at"}]
    },
    "by_buyer": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "loco/bricklink.buyer", "op": "eq", "value": "ada_bricks"}
    },
    "all_lines": {
      "collection": "loco/bricklink.store:order_items"
    },
    "lines_by_item": {
      "collection": "loco/bricklink.store:order_items",
      "where": {"field": "loco/bricklink.item_no", "op": "eq", "value": "3001"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.after.error" == "query failed"
jsonpath "$.data.after.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.after.diagnostics[0].message" contains "op 'gt'"
jsonpath "$.data.stamped.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.stamped.diagnostics[0].message" contains "ordering by $created_at"
jsonpath "$.data.by_buyer.error" == "query failed"
jsonpath "$.data.by_buyer.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.by_buyer.diagnostics[0].path" == "by_buyer"
jsonpath "$.data.by_buyer.diagnostics[0].message" contains "filtered by status or $id only"
jsonpath "$.data.all_lines.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.all_lines.diagnostics[0].path" == "all_lines"
jsonpath "$.data.all_lines.diagnostics[0].message" contains "order_items needs an order_id filter"
jsonpath "$.data.lines_by_item.diagnostics[0].kind" == "unsupported"
jsonpath "$.data.lines_by_item.diagnostics[0].message" contains "order_items needs an order_id filter"

# ---------------------------------------------------------------------------
# A credential BrickLink does not know. BrickLink answers HTTP 200 with
# meta.code 401. The caller gets 502 with BrickLink's message, and nothing
# the request carried: the fixture echoes the consumer key, and the source
# removes it.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/list
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: other
HTTP 502
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "upstream 401: BAD_OAUTH_REQUEST: consumer key [redacted] is not registered"
body not contains "unregistered-key-7f3a"
body not contains "other-token-9c1e"
body not contains "oauth_"
body not contains "127.0.0.1"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: other
Content-Type: application/json
{
  "queries": {
    "pending": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "loco/bricklink.status", "op": "eq", "value": "PENDING"}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.pending.error" == "query failed"
jsonpath "$.data.pending.diagnostics[0].kind" == "upstream"
jsonpath "$.data.pending.diagnostics[0].message" == "upstream 401: BAD_OAUTH_REQUEST: consumer key [redacted] is not registered"
body not contains "unregistered-key-7f3a"

# A wrong secret signs a request the server rejects: the same 502 shape.
PUT http://127.0.0.1:{{port}}/config/secret/alice/brocks/other/loco%2Fbricklink.store:consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "fixture-consumer-key"}
HTTP 200

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/get/29471234
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: other
HTTP 502
[Asserts]
jsonpath "$.error" == "upstream 401: BAD_OAUTH_REQUEST: SIGNATURE_INVALID: [redacted]"
body not contains "other-consumer-secret"

# ---------------------------------------------------------------------------
# A base_url that misses the API. The server answers a bare HTTP 404 with no
# BrickLink envelope. That is an upstream failure, never "no orders": a
# pulling app must not conclude there is nothing to pick. The 404 body is
# not passed on.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/variable/alice/brocks/dev/loco%2Fbricklink.store:base_url
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{bricklink}}/nope"}
HTTP 200

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/list
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 502
[Asserts]
jsonpath "$.error" == "upstream 404: Not Found"
body not contains "nope"
body not contains "Cannot GET"

GET http://127.0.0.1:{{port}}/data/loco%2Fbricklink.store:orders/get/29471234
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
HTTP 502
[Asserts]
jsonpath "$.error" == "upstream 404: Not Found"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/brocks
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "pending": {
      "collection": "loco/bricklink.store:orders",
      "where": {"field": "loco/bricklink.status", "op": "eq", "value": "PENDING"}
    },
    "lines": {
      "collection": "loco/bricklink.store:order_items",
      "where": {"field": "loco/bricklink.order_id", "op": "in", "value": ["29471234"]}
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.pending.error" == "query failed"
jsonpath "$.data.pending.diagnostics[0].kind" == "upstream"
jsonpath "$.data.pending.diagnostics[0].message" == "upstream 404: Not Found"
jsonpath "$.data.lines.error" == "query failed"
jsonpath "$.data.lines.diagnostics[0].kind" == "upstream"
body not contains "nope"
