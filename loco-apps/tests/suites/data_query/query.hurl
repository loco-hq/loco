# POST /data/query: named, batched, cursor-paged reads (docs/query.md).
#
# Fixtures: project testuser/testapp, site "www" pinned to 1-dev.
#   1-dev — lot: batch_id, item_no (string), qty (integer), price (float).
#           secret: note. public_read grants read on lot only.
#           Depends on acme/crm@1.0.
#   acme/crm 1.0 — contacts: email. No public grant.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "testuser", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# --- Records: batch A has five lots (one with qty 0), batch B has one. ---

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"batch_id": "A", "item_no": "3003", "qty": 4, "price": 0.5}
HTTP 201

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"batch_id": "A", "item_no": "3001", "qty": 10, "price": 2}
HTTP 201

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"batch_id": "A", "item_no": "3002", "qty": 0, "price": 1.25}
HTTP 201

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"batch_id": "A", "item_no": "3005", "qty": 1}
HTTP 201

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"batch_id": "A", "item_no": "3004", "qty": 7, "price": 3.0}
HTTP 201

POST http://127.0.0.1:{{port}}/data/lot/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"batch_id": "B", "item_no": "3001", "qty": 5, "price": 2}
HTTP 201

POST http://127.0.0.1:{{port}}/data/secret/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"note": "members only"}
HTTP 201

# --- A filtered, ordered, limited query, beside queries that fail. ---
# The failing ones get an error result each; `lots` still runs and the
# batch is a 200.

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "lots": {
      "collection": "lot",
      "where": { "and": [
        { "field": "batch_id", "op": "eq", "value": "A" },
        { "field": "qty", "op": "gt", "value": 0 }
      ] },
      "fields": ["item_no", "qty"],
      "order": [{ "field": "item_no" }],
      "limit": 2
    },
    "typo": {
      "collection": "lot",
      "where": { "field": "qyt", "op": "gt", "value": 0 }
    },
    "mismatch": {
      "collection": "lot",
      "where": { "and": [
        { "field": "batch_id", "op": "eq", "value": "A" },
        { "field": "qty", "op": "gt", "value": "3" }
      ] },
      "order": [{ "field": "nope", "dir": "down" }]
    },
    "missing": { "collection": "lots" },
    "too_many": { "collection": "lot", "limit": 501 }
  }
}
HTTP 200
[Captures]
page2: jsonpath "$.data.lots.cursor"
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.lots.records" count == 2
jsonpath "$.data.lots.records[0].fields.item_no" == "3001"
jsonpath "$.data.lots.records[1].fields.item_no" == "3003"
jsonpath "$.data.lots.records[0].fields.qty" == 10
# `fields` projects the fields map; system fields are always there.
jsonpath "$.data.lots.records[0].fields.batch_id" not exists
jsonpath "$.data.lots.records[0].fields.price" not exists
jsonpath "$.data.lots.records[0].id" exists
jsonpath "$.data.lots.records[0].created_at" exists
jsonpath "$.data.lots.cursor" isString
jsonpath "$.data.lots.error" not exists
jsonpath "$.data.typo.error" == "query failed"
jsonpath "$.data.typo.records" not exists
jsonpath "$.data.typo.diagnostics" count == 1
jsonpath "$.data.typo.diagnostics[0].kind" == "unknown_field"
jsonpath "$.data.typo.diagnostics[0].severity" == "error"
jsonpath "$.data.typo.diagnostics[0].path" == "typo/where/field"
# Every problem in a query is reported, not just the first.
jsonpath "$.data.mismatch.diagnostics" count == 3
jsonpath "$.data.mismatch.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.data.mismatch.diagnostics[0].path" == "mismatch/where/and/1/value"
jsonpath "$.data.mismatch.diagnostics[1].kind" == "invalid_query"
jsonpath "$.data.mismatch.diagnostics[1].path" == "mismatch/order/0/dir"
jsonpath "$.data.mismatch.diagnostics[2].kind" == "unknown_field"
jsonpath "$.data.mismatch.diagnostics[2].path" == "mismatch/order/0/field"
jsonpath "$.data.missing.diagnostics[0].kind" == "unknown_collection"
jsonpath "$.data.missing.diagnostics[0].path" == "missing/collection"
jsonpath "$.data.too_many.diagnostics[0].kind" == "limit_exceeded"
jsonpath "$.data.too_many.diagnostics[0].path" == "too_many/limit"

# --- The second page via the cursor: the last two, and no further cursor. ---
# `limit` and `fields` may change between pages.

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "lots": {
      "collection": "lot",
      "where": { "and": [
        { "field": "batch_id", "op": "eq", "value": "A" },
        { "field": "qty", "op": "gt", "value": 0 }
      ] },
      "order": [{ "field": "item_no", "dir": "asc" }],
      "limit": 5,
      "cursor": "{{page2}}"
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.lots.records" count == 2
jsonpath "$.data.lots.records[0].fields.item_no" == "3004"
jsonpath "$.data.lots.records[1].fields.item_no" == "3005"
jsonpath "$.data.lots.records[0].fields.batch_id" == "A"
jsonpath "$.data.lots.cursor" == null

# The same cursor with a different `where` is refused, not reinterpreted.
POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "lots": {
      "collection": "lot",
      "where": { "field": "batch_id", "op": "eq", "value": "B" },
      "order": [{ "field": "item_no" }],
      "cursor": "{{page2}}"
    },
    "garbled": { "collection": "lot", "cursor": "not-a-cursor" }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.lots.diagnostics[0].kind" == "cursor_mismatch"
jsonpath "$.data.lots.diagnostics[0].path" == "lots/cursor"
jsonpath "$.data.garbled.diagnostics[0].kind" == "invalid_query"

# --- The other ops, and descending order. ---
# `price` is a float field, so the integer 2 matches 2 and 2.0 alike.

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "unpriced": {
      "collection": "lot",
      "where": { "field": "price", "op": "exists", "value": false }
    },
    "two": {
      "collection": "lot",
      "where": { "field": "price", "op": "eq", "value": 2 },
      "order": [{ "field": "batch_id", "dir": "desc" }]
    },
    "picked": {
      "collection": "lot",
      "where": { "and": [
        { "field": "item_no", "op": "in", "value": ["3002", "3004"] },
        { "not": { "field": "qty", "op": "lte", "value": 0 } }
      ] }
    },
    "by_qty": {
      "collection": "lot",
      "fields": ["qty"],
      "order": [{ "field": "qty", "dir": "desc" }],
      "limit": 3
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.unpriced.records" count == 1
jsonpath "$.data.unpriced.records[0].fields.item_no" == "3005"
jsonpath "$.data.two.records" count == 2
jsonpath "$.data.two.records[0].fields.batch_id" == "B"
jsonpath "$.data.two.records[1].fields.batch_id" == "A"
jsonpath "$.data.picked.records" count == 1
jsonpath "$.data.picked.records[0].fields.item_no" == "3004"
jsonpath "$.data.by_qty.records[0].fields.qty" == 10
jsonpath "$.data.by_qty.records[1].fields.qty" == 7
jsonpath "$.data.by_qty.records[2].fields.qty" == 5
jsonpath "$.data.by_qty.cursor" isString

# --- Public: `lot` is granted, `secret` is not. The batch still runs. ---

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "lots": { "collection": "lot", "where": { "field": "batch_id", "op": "eq", "value": "B" } },
    "secrets": { "collection": "secret" },
    "contacts": { "collection": "acme/crm.contacts" }
  }
}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.lots.records" count == 1
jsonpath "$.data.secrets.error" == "query failed"
jsonpath "$.data.secrets.diagnostics[0].kind" == "forbidden"
jsonpath "$.data.secrets.diagnostics[0].path" == "secrets"
jsonpath "$.data.contacts.diagnostics[0].kind" == "forbidden"

# --- Names: bare means this project; a dependency's is qualified. ---
# Records under acme/crm.contacts are keyed apart from any testapp
# collection, and nothing has written any here, so the page is empty — what
# matters is that the name resolves. A bare `email` on acme's collection is
# this project's `email`, and testapp declares none on contacts.

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "contacts": {
      "collection": "acme/crm.contacts",
      "where": { "field": "acme/crm.email", "op": "eq", "value": "a@b.c" }
    },
    "bare_email": {
      "collection": "acme/crm.contacts",
      "where": { "field": "email", "op": "eq", "value": "a@b.c" }
    },
    "bare_contacts": { "collection": "contacts" },
    "not_a_dep": { "collection": "other/crm.contacts" },
    "self_qualified": {
      "collection": "testuser/testapp.lot",
      "where": { "field": "testuser/testapp.batch_id", "op": "eq", "value": "B" }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.contacts.records" count == 0
jsonpath "$.data.contacts.cursor" == null
jsonpath "$.data.bare_email.diagnostics[0].kind" == "unknown_field"
jsonpath "$.data.bare_email.diagnostics[0].path" == "bare_email/where/field"
jsonpath "$.data.bare_contacts.diagnostics[0].kind" == "unknown_collection"
jsonpath "$.data.not_a_dep.diagnostics[0].kind" == "unknown_collection"
jsonpath "$.data.self_qualified.records" count == 1
jsonpath "$.data.self_qualified.records[0].fields.item_no" == "3001"

# --- System fields: `$`-prefixed in where and order, never in fields. ---

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{
  "queries": {
    "mine": {
      "collection": "lot",
      "where": { "field": "$created_by", "op": "eq", "value": "testuser" },
      "order": [{ "field": "$id", "dir": "desc" }],
      "limit": 500
    },
    "projected": { "collection": "lot", "fields": ["$id"] },
    "unknown": { "collection": "lot", "where": { "field": "$nope", "op": "exists", "value": true } }
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.mine.records" count == 6
jsonpath "$.data.projected.diagnostics[0].kind" == "invalid_query"
jsonpath "$.data.projected.diagnostics[0].path" == "projected/fields/0"
jsonpath "$.data.unknown.diagnostics[0].kind" == "unknown_field"

# --- Only a body that is not a batch at all fails the request. ---

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"collection": "lot"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
{"queries": {"Not-A-Name": {"collection": "lot"}}}
HTTP 400

# Site-level failures still fail the whole request.
POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: testuser/testapp
X-Site-Id: nosuch
{"queries": {}}
HTTP 404

# --- GET /data/{collection}/list is unchanged: every record, no paging. ---

GET http://127.0.0.1:{{port}}/data/lot/list
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data" count == 6
