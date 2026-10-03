# Declared actions and the /actions runner (issue #47).
#
# A draft declares an action with its params on the document; a duplicate
# param name and an unknown param type are 400; a published version refuses
# the write; a version copy carries the action; a version delete removes it.
# GET /actions on a site returns the action with its params, in declared
# order, and a consumer sees a dependency's action qualified. POST runs the
# fixture handler registered
# for alice/fixture echo. Bad input is the /data 400 and does not run it. A
# declared action with no handler is 501. An unknown name is 404. Public and
# a caller who cannot write data get the same refusal as POST /data/.../add.
# The same action name declared on another project does not run this handler.
#
# No fixtures. alice creates alice/fixture (owns the handler), alice/shop
# (the consumer), and alice/other (a same-named declaration with no handler).

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "fixture", "label": "Fixture", "description": "Owns echo"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Consumer"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other", "description": "Same name, no handler"}
HTTP 201

# ---------------------------------------------------------------------------
# Draft declares an action with its params, and lists it on /schema.
# Params stay in the order the body declared them.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "echo",
  "label": "Echo",
  "description": "Returns {ran, dataset_id, caller, input}.",
  "params": [
    {
      "name": "size",
      "type": "string",
      "label": "Size",
      "description": "Shirt size",
      "required": true,
      "options": [
        {"value": "s", "label": "Small"},
        {"value": "m", "label": "Medium"}
      ]
    },
    {"name": "qty", "type": "integer", "label": "Qty", "required": true},
    {"name": "note", "type": "string", "label": "Note"}
  ]
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "echo"
jsonpath "$.data.label" == "Echo"
jsonpath "$.data.description" == "Returns {ran, dataset_id, caller, input}."
jsonpath "$.data.project" == "alice/fixture"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.params" count == 3
jsonpath "$.data.params[0].name" == "size"
jsonpath "$.data.params[0].type" == "string"
jsonpath "$.data.params[0].required" == true
jsonpath "$.data.params[0].options[0].value" == "s"
jsonpath "$.data.params[0].options[1].label" == "Medium"
jsonpath "$.data.params[1].name" == "qty"
jsonpath "$.data.params[1].type" == "integer"
jsonpath "$.data.params[2].name" == "note"
jsonpath "$.data.params[2].required" == false
jsonpath "$.data.params[2].options" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/echo
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Echo"
jsonpath "$.data.params[0].name" == "size"
jsonpath "$.data.params[1].name" == "qty"
jsonpath "$.data.params[2].name" == "note"

# A duplicate name, a name that is not a slug, an unknown type, and options
# on a non-string are 400, and none of those actions is stored.
POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "dup",
  "label": "Dup",
  "params": [
    {"name": "qty", "type": "integer", "label": "Qty"},
    {"name": "qty", "type": "string", "label": "Qty again"}
  ]
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "param name 'qty' is declared more than once"

GET http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/dup
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "bad_name", "label": "Bad", "params": [{"name": "Qty", "type": "string", "label": "Qty"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "must be a slug"

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "bad_type", "label": "Bad", "params": [{"name": "tags", "type": "list", "label": "Tags"}]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "unknown field type 'list'"

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "bad_options",
  "label": "Bad",
  "params": [
    {"name": "qty", "type": "integer", "label": "Qty", "options": [{"value": "1", "label": "One"}]}
  ]
}
HTTP 400
[Asserts]
jsonpath "$.error" contains "only meaningful for type string"

GET http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "echo"
jsonpath "$.data[0].project" == "alice/fixture"
jsonpath "$.data[0].params" count == 3

# A label write leaves the params. A write that names params replaces the list.
PUT http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/echo
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Echo input"}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Echo input"
jsonpath "$.data.description" == "Returns {ran, dataset_id, caller, input}."
jsonpath "$.data.params" count == 3
jsonpath "$.data.params[0].name" == "size"

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "swap", "label": "Swap", "params": [{"name": "note", "type": "string", "label": "Note"}]}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/swap
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "params": [
    {"name": "qty", "type": "integer", "label": "Qty", "required": true},
    {"name": "size", "type": "string", "label": "Size"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Swap"
jsonpath "$.data.params" count == 2
jsonpath "$.data.params[0].name" == "qty"
jsonpath "$.data.params[1].name" == "size"

DELETE http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/swap
Authorization: Bearer {{token}}
HTTP 200

# ---------------------------------------------------------------------------
# Copy carries the action, params included. The published target then
# refuses a write, including one that would replace the params.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/config/version/alice/fixture
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/fixture/1.0.0/action/echo
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Echo input"
jsonpath "$.data.version" == "1.0.0"
jsonpath "$.data.params" count == 3
jsonpath "$.data.params[0].name" == "size"
jsonpath "$.data.params[0].options[0].value" == "s"
jsonpath "$.data.params[1].name" == "qty"
jsonpath "$.data.params[2].name" == "note"

POST http://127.0.0.1:{{port}}/schema/alice/fixture/1.0.0/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other_action", "label": "Other"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

PUT http://127.0.0.1:{{port}}/schema/alice/fixture/1.0.0/action/echo
Authorization: Bearer {{token}}
Content-Type: application/json
{"params": []}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

GET http://127.0.0.1:{{port}}/schema/alice/fixture/1.0.0/action/echo
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.params" count == 3

# ---------------------------------------------------------------------------
# The site list, and a consumer. Params are the owner's list. A bare name
# on the consumer is the consumer's own action, which does not exist.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/fixture@0.0.1-dev"]}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "echo"
jsonpath "$.data[0].project" == "alice/fixture"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/action/alice%2Ffixture.echo
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "alice/fixture"
jsonpath "$.data.name" == "echo"
jsonpath "$.data.params" count == 3
jsonpath "$.data.params[0].name" == "size"
jsonpath "$.data.params[1].name" == "qty"
jsonpath "$.data.params[2].name" == "note"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/action/echo
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/actions
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "echo"
jsonpath "$.data[0].project" == "alice/fixture"
jsonpath "$.data[0].label" == "Echo input"
jsonpath "$.data[0].params" count == 3
jsonpath "$.data[0].params[0].name" == "size"
jsonpath "$.data[0].params[0].options[0].value" == "s"
jsonpath "$.data[0].params[1].name" == "qty"
jsonpath "$.data[0].params[1].type" == "integer"
jsonpath "$.data[0].params[1].required" == true
jsonpath "$.data[0].params[2].name" == "note"

GET http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.params[0].name" == "size"
jsonpath "$.data.params[2].name" == "note"

GET http://127.0.0.1:{{port}}/actions
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].project" == "alice/fixture"
jsonpath "$.data[0].name" == "echo"
jsonpath "$.data[0].params" count == 3
jsonpath "$.data[0].params[0].name" == "size"
jsonpath "$.data[0].params[2].name" == "note"

GET http://127.0.0.1:{{port}}/actions/alice%2Ffixture.echo
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.project" == "alice/fixture"
jsonpath "$.data.params" count == 3

GET http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" contains "action not found: echo"

# ---------------------------------------------------------------------------
# POST runs the fixture handler. Bad input is the /data 400 and does not.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 2, "size": "m", "note": "hi"}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.ran" == true
jsonpath "$.data.dataset_id" == "alice/fixture/dev"
jsonpath "$.data.caller" == "alice"
jsonpath "$.data.input.qty" == 2
jsonpath "$.data.input.size" == "m"
jsonpath "$.data.input.note" == "hi"
jsonpath "$.diagnostics" not exists

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"size": "m"}}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "validation failed"
jsonpath "$.data" not exists
jsonpath "$.diagnostics[0].severity" == "error"
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "qty"
jsonpath "$.diagnostics[0].message" contains "param 'qty' is required"

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": "two", "size": "m"}}
HTTP 400
[Asserts]
jsonpath "$.error" == "validation failed"
jsonpath "$.data" not exists
jsonpath "$.diagnostics[0].kind" == "type_mismatch"
jsonpath "$.diagnostics[0].path" == "qty"
jsonpath "$.diagnostics[0].message" contains "param 'qty' expected type 'integer'"

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "xl"}}
HTTP 400
[Asserts]
jsonpath "$.error" == "validation failed"
jsonpath "$.data" not exists
jsonpath "$.diagnostics[0].kind" == "invalid_option"
jsonpath "$.diagnostics[0].path" == "size"
jsonpath "$.diagnostics[0].message" contains "param 'size' must be one of"

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "m", "extra": "no"}}
HTTP 400
[Asserts]
jsonpath "$.error" == "validation failed"
jsonpath "$.data" not exists
jsonpath "$.diagnostics[0].kind" == "unknown_field"
jsonpath "$.diagnostics[0].path" == "extra"
jsonpath "$.diagnostics[0].message" contains "param 'extra' is not declared in action 'echo'"

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "", "note": "x"}}
HTTP 400
[Asserts]
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "size"
jsonpath "$.data" not exists

# A consumer runs the owner's handler, against the consumer's dataset.
POST http://127.0.0.1:{{port}}/actions/alice%2Ffixture.echo
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "s"}}
HTTP 200
[Asserts]
jsonpath "$.data.ran" == true
jsonpath "$.data.dataset_id" == "alice/shop/dev"
jsonpath "$.data.caller" == "alice"
jsonpath "$.data.input.qty" == 1

# Bare echo on the shop is not the fixture action, so the handler does not run.
POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "s"}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "action not found"
jsonpath "$.data" not exists

# ---------------------------------------------------------------------------
# No handler is 501, naming the owning project. Unknown is 404.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "noop", "label": "Noop", "description": "Declared, no handler."}
HTTP 201

POST http://127.0.0.1:{{port}}/actions/noop
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 501
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "no handler for action alice/fixture.noop"
jsonpath "$.data" not exists

POST http://127.0.0.1:{{port}}/actions/missing
Authorization: Bearer {{token}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "action not found: missing"

# ---------------------------------------------------------------------------
# Same bare name on a project that does not own the handler. Empty input
# would fail fixture's required params, so a 501 (not a 400, not a run)
# is the other project's own declaration.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/other/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "echo", "label": "Other echo"}
HTTP 201

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{token}}
X-Project-Id: alice/other
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 501
[Asserts]
jsonpath "$.error" contains "no handler for action alice/other.echo"
jsonpath "$.data" not exists
body not contains "\"ran\""

# ---------------------------------------------------------------------------
# Public, and a caller who cannot write, get the /data add refusal.
# A public create grant still cannot run the action. An editor can.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "items", "label": "Items"}
HTTP 201

POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/actions/echo
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"
jsonpath "$.data" not exists

GET http://127.0.0.1:{{port}}/actions
X-Project-Id: alice/fixture
X-Site-Id: dev
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "bob", "password": "password"}
HTTP 200
[Captures]
bob: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/data/items/add
Authorization: Bearer {{bob}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{bob}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "m"}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"
jsonpath "$.data" not exists

POST http://127.0.0.1:{{port}}/config/member/alice/fixture
Authorization: Bearer {{token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201
[Asserts]
jsonpath "$.data.role" == "editor"

POST http://127.0.0.1:{{port}}/actions/echo
Authorization: Bearer {{bob}}
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "m"}}
HTTP 200
[Asserts]
jsonpath "$.data.ran" == true
jsonpath "$.data.caller" == "bob"
jsonpath "$.data.dataset_id" == "alice/fixture/dev"

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "public_items",
  "label": "Public items",
  "collections": [{"collection": "items", "read": true, "create": true}]
}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"public_permission_sets": ["public_items"]}
HTTP 200

POST http://127.0.0.1:{{port}}/data/items/add
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.owner" == "public"

GET http://127.0.0.1:{{port}}/actions
X-Project-Id: alice/fixture
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data[*].name" includes "echo"

POST http://127.0.0.1:{{port}}/actions/echo
X-Project-Id: alice/fixture
X-Site-Id: dev
Content-Type: application/json
{"input": {"qty": 1, "size": "m"}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"
jsonpath "$.data" not exists

# ---------------------------------------------------------------------------
# Deleting an action removes the document, params included. A recreate
# starts from the body it is given.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "temp", "label": "Temp", "params": [{"name": "qty", "type": "integer", "label": "Qty", "required": true}]}
HTTP 201

DELETE http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/temp
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/temp
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "temp", "label": "Temp again"}
HTTP 201
[Asserts]
jsonpath "$.data.params" count == 0

DELETE http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/temp
Authorization: Bearer {{token}}
HTTP 200

# ---------------------------------------------------------------------------
# Version delete removes what remains. The dev site pins 0.0.1-dev, and the
# consumer depends on it, so both move to the copy first.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/site/alice/fixture/dev
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0"}
HTTP 200

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/fixture@1.0.0"]}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/version/alice/fixture/0.0.1-dev
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/echo
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/config/version/alice/fixture
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1-dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/fixture/0.0.1-dev/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# The copy still has the action, params included, in declared order.
GET http://127.0.0.1:{{port}}/schema/alice/fixture/1.0.0/action/echo
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Echo input"
jsonpath "$.data.params" count == 3
jsonpath "$.data.params[0].name" == "size"
jsonpath "$.data.params[1].name" == "qty"
jsonpath "$.data.params[2].name" == "note"
