# Connection secrets and variables are keyed by integration (issue #126).
#
# alice/pkg declares type warehouse (token required, consumer_key optional,
# region defaulting to us, action read). It also declares integration store
# of that type. alice/shop depends on the package, declares sf_east and
# sf_west of the same type, and a loose secret also named token.
#
# Values live on the shop's dev dataset. Each integration reads its own
# row. The loose token is a different row. The runner registers
# (alice/pkg, warehouse, read); unread has no handler.
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
    {"name": "token", "label": "Token", "required": true},
    {"name": "consumer_key", "label": "Consumer key", "required": false}
  ],
  "variables": [
    {"name": "region", "label": "Region", "required": false, "default": "us"}
  ],
  "actions": [
    {"name": "read", "label": "Read", "description": "Returns this connection's values."},
    {"name": "unread", "label": "Unread", "description": "No handler."}
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "store", "label": "Store", "type": "warehouse"}
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
{"name": "sf_east", "label": "East", "type": "alice/pkg.warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "sf_west", "label": "West", "type": "alice/pkg.warehouse"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "token", "label": "Loose token", "required": true}
HTTP 201

# A name the type does not declare is refused. It does not fall through
# to the loose secret of the same bare name.
PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/sf_east:nope
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "nope"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "not declared"

# ---------------------------------------------------------------------------
# Lists before any value is stored. Loose rows omit integration and sort
# first. Connection rows follow, one per integration per declaration,
# by canonical integration then name.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 7
jsonpath "$.data[0].name" == "token"
jsonpath "$.data[0].project" == "alice/shop"
jsonpath "$.data[0].integration" not exists
jsonpath "$.data[0].set" == false
jsonpath "$.data[0].value" not exists
jsonpath "$.data[1].name" == "consumer_key"
jsonpath "$.data[1].project" == "alice/pkg"
jsonpath "$.data[1].integration" == "alice/pkg.store"
jsonpath "$.data[1].set" == false
jsonpath "$.data[2].name" == "token"
jsonpath "$.data[2].project" == "alice/pkg"
jsonpath "$.data[2].integration" == "alice/pkg.store"
jsonpath "$.data[3].name" == "consumer_key"
jsonpath "$.data[3].project" == "alice/shop"
jsonpath "$.data[3].integration" == "sf_east"
jsonpath "$.data[4].name" == "token"
jsonpath "$.data[4].integration" == "sf_east"
jsonpath "$.data[5].name" == "consumer_key"
jsonpath "$.data[5].integration" == "sf_west"
jsonpath "$.data[6].name" == "token"
jsonpath "$.data[6].integration" == "sf_west"
jsonpath "$.data[6].project" == "alice/shop"

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].name" == "region"
jsonpath "$.data[0].project" == "alice/pkg"
jsonpath "$.data[0].integration" == "alice/pkg.store"
jsonpath "$.data[0].set" == false
jsonpath "$.data[0].source" == "default"
jsonpath "$.data[0].value" == "us"
jsonpath "$.data[1].name" == "region"
jsonpath "$.data[1].project" == "alice/shop"
jsonpath "$.data[1].integration" == "sf_east"
jsonpath "$.data[1].source" == "default"
jsonpath "$.data[1].value" == "us"
jsonpath "$.data[2].integration" == "sf_west"
jsonpath "$.data[2].source" == "default"
jsonpath "$.data[2].value" == "us"

# The loose token does not satisfy sf_east, and the 400 does not reveal it.
PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "loose-token"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "token"
jsonpath "$.data.project" == "alice/shop"
jsonpath "$.data.integration" not exists
jsonpath "$.data.set" == true
jsonpath "$.data.value" not exists
body not contains "loose-token"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/sf_west:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "west-token"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "token"
jsonpath "$.data.project" == "alice/shop"
jsonpath "$.data.integration" == "sf_west"
jsonpath "$.data.set" == true
jsonpath "$.data.value" not exists
body not contains "west-token"

POST http://127.0.0.1:{{port}}/actions/sf_east:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.data" not exists
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "token"
jsonpath "$.diagnostics[0].message" == "secret 'token' is not set"
body not contains "loose-token"
body not contains "west-token"
body not contains "ran"

# No handler is 501 before the required check, so the missing token is
# not named and the loose plaintext stays out of the body.
POST http://127.0.0.1:{{port}}/actions/sf_east:unread
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 501
[Asserts]
jsonpath "$.error" == "no handler for action alice/pkg.warehouse.unread"
body not contains "required configuration"
body not contains "loose-token"
body not contains "west-token"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/sf_east:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "east-token"}
HTTP 200
[Asserts]
jsonpath "$.data.integration" == "sf_east"
jsonpath "$.data.name" == "token"
jsonpath "$.data.project" == "alice/shop"
body not contains "east-token"

POST http://127.0.0.1:{{port}}/actions/sf_east:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.integration" == "sf_east"
jsonpath "$.data.token" == "east-token"
jsonpath "$.data.consumer_key" == null
jsonpath "$.data.region" == "us"
jsonpath "$.data.loose.error" contains "not declared"
body not contains "loose-token"
body not contains "west-token"

POST http://127.0.0.1:{{port}}/actions/sf_west:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.integration" == "sf_west"
jsonpath "$.data.token" == "west-token"
jsonpath "$.data.consumer_key" == null
jsonpath "$.data.region" == "us"
body not contains "east-token"
body not contains "loose-token"

# The shop sets the dependency's integration. The row id is
# alice/pkg.store:consumer_key, the same shape as
# loco/bricklink.store:consumer_key.
PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fpkg.store:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "store-token"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "token"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.integration" == "alice/pkg.store"
body not contains "store-token"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fpkg.store:consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "store-key"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "consumer_key"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.integration" == "alice/pkg.store"
body not contains "store-key"

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.store:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.integration" == "alice/pkg.store"
jsonpath "$.data.token" == "store-token"
jsonpath "$.data.consumer_key" == "store-key"
jsonpath "$.data.region" == "us"
body not contains "east-token"
body not contains "west-token"
body not contains "loose-token"

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 7
jsonpath "$.data[0].name" == "token"
jsonpath "$.data[0].integration" not exists
jsonpath "$.data[0].set" == true
jsonpath "$.data[1].integration" == "alice/pkg.store"
jsonpath "$.data[1].name" == "consumer_key"
jsonpath "$.data[1].set" == true
jsonpath "$.data[2].integration" == "alice/pkg.store"
jsonpath "$.data[2].name" == "token"
jsonpath "$.data[2].set" == true
jsonpath "$.data[3].integration" == "sf_east"
jsonpath "$.data[3].name" == "consumer_key"
jsonpath "$.data[3].set" == false
jsonpath "$.data[4].integration" == "sf_east"
jsonpath "$.data[4].set" == true
jsonpath "$.data[5].integration" == "sf_west"
jsonpath "$.data[5].set" == false
jsonpath "$.data[6].integration" == "sf_west"
jsonpath "$.data[6].set" == true
body not contains "loose-token"
body not contains "east-token"
body not contains "west-token"
body not contains "store-token"
body not contains "store-key"

# A stored region wins, including empty. Unset integrations keep the default.
PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/sf_east:region
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "eu"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "region"
jsonpath "$.data.integration" == "sf_east"
jsonpath "$.data.project" == "alice/shop"
jsonpath "$.data.set" == true
jsonpath "$.data.source" == "value"
jsonpath "$.data.value" == "eu"

POST http://127.0.0.1:{{port}}/actions/sf_east:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.region" == "eu"
jsonpath "$.data.token" == "east-token"

POST http://127.0.0.1:{{port}}/actions/sf_west:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.region" == "us"

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].integration" == "alice/pkg.store"
jsonpath "$.data[0].source" == "default"
jsonpath "$.data[0].value" == "us"
jsonpath "$.data[1].integration" == "sf_east"
jsonpath "$.data[1].source" == "value"
jsonpath "$.data[1].value" == "eu"
jsonpath "$.data[1].set" == true
jsonpath "$.data[2].integration" == "sf_west"
jsonpath "$.data[2].source" == "default"
jsonpath "$.data[2].value" == "us"

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/sf_east:region
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": ""}
HTTP 200
[Asserts]
jsonpath "$.data.source" == "value"
jsonpath "$.data.value" == ""
jsonpath "$.data.set" == true

POST http://127.0.0.1:{{port}}/actions/sf_east:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.region" == ""

# DELETE strips a `{project}.` prefix and leaves the colon, so the
# qualified form removes the dependency row. The token on that
# integration stays.
DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fshop.alice%2Fpkg.store:consumer_key
Authorization: Bearer {{token}}
HTTP 200

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.store:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.integration" == "alice/pkg.store"
jsonpath "$.data.consumer_key" == null
jsonpath "$.data.token" == "store-token"
body not contains "store-key"

# A qualified name for this project's own integration is the bare row.
PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fshop.sf_east:token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "east-via-qualified"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "token"
jsonpath "$.data.integration" == "sf_east"
jsonpath "$.data.project" == "alice/shop"
body not contains "east-via-qualified"

POST http://127.0.0.1:{{port}}/actions/sf_east:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.token" == "east-via-qualified"
body not contains "loose-token"

DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fshop.sf_east:token
Authorization: Bearer {{token}}
HTTP 200

POST http://127.0.0.1:{{port}}/actions/sf_east:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 400
[Asserts]
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.diagnostics[0].message" == "secret 'token' is not set"
body not contains "east-via-qualified"
body not contains "loose-token"
body not contains "west-token"

POST http://127.0.0.1:{{port}}/actions/sf_west:read
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.data.token" == "west-token"
jsonpath "$.data.region" == "us"
