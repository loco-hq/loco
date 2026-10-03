# Secret and variable declarations on a version (issue #100).
#
# Both are metadata only: a name the version needs, never the value. A draft
# declares them and reads them back; a published version refuses the write; a
# consumer's list shows a dependency's declaration with its project; a version
# copy carries them; a version delete removes them. A variable round-trips its
# default. A secret body with `default` or `value` is a 400, and so is a
# secret and a variable that share a name in one version.
#
# No fixtures. alice creates alice/bricklink (the package) and alice/store
# (the consumer). Each bootstraps 0.0.1-dev, a dev dataset, and a dev site.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "bricklink", "label": "BrickLink", "description": "Package"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "store", "label": "Store", "description": "Consumer"}
HTTP 201

# ---------------------------------------------------------------------------
# Draft declares one of each and reads it back.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "consumer_key",
  "label": "Consumer key",
  "description": "BrickLink OAuth consumer key",
  "required": true
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.name" == "consumer_key"
jsonpath "$.data.label" == "Consumer key"
jsonpath "$.data.description" == "BrickLink OAuth consumer key"
jsonpath "$.data.required" == true
jsonpath "$.data.project" == "alice/bricklink"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.default" not exists
jsonpath "$.data.value" not exists

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Consumer key"
jsonpath "$.data.required" == true

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "consumer_key"
jsonpath "$.data[0].project" == "alice/bricklink"

PUT http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "OAuth consumer key"}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "OAuth consumer key"
jsonpath "$.data.required" == true
jsonpath "$.data.description" == "BrickLink OAuth consumer key"

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "api_base",
  "label": "API base",
  "description": "BrickLink API origin",
  "required": false,
  "default": "https://api.bricklink.com/api/store/v1"
}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "api_base"
jsonpath "$.data.default" == "https://api.bricklink.com/api/store/v1"
jsonpath "$.data.required" == false
jsonpath "$.data.project" == "alice/bricklink"
jsonpath "$.data.version" == "0.0.1-dev"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable/api_base
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.default" == "https://api.bricklink.com/api/store/v1"

PUT http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable/api_base
Authorization: Bearer {{token}}
Content-Type: application/json
{"default": "https://fixture.test/api", "required": true}
HTTP 200
[Asserts]
jsonpath "$.data.default" == "https://fixture.test/api"
jsonpath "$.data.required" == true
jsonpath "$.data.label" == "API base"

# An omitted default is empty, not a value the package shipped.
POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "region", "label": "Region"}
HTTP 201
[Asserts]
jsonpath "$.data.default" == ""
jsonpath "$.data.required" == false

# ---------------------------------------------------------------------------
# A secret body carrying default or value is a 400, and nothing is stored.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "token_secret", "label": "Token", "default": "shipped-in-the-package"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "default"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/token_secret
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "token_value", "value": "super-secret"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "value"

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "both", "default": "d", "value": "v"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "default"
jsonpath "$.error" contains "value"

# The same refusal on update, and the stored declaration is unchanged.
PUT http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Should not stick", "value": "nope"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "value"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "OAuth consumer key"

# ---------------------------------------------------------------------------
# One version cannot declare a secret and a variable under the same name.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "consumer_key", "label": "Not a variable"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "consumer_key"

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "api_base", "label": "Not a secret"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "api_base"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable/api_base
Authorization: Bearer {{token}}
HTTP 200

# ---------------------------------------------------------------------------
# A consumer's list shows the dependency's declaration, with its project.
# A bare name is the consumer's own, which has neither.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/bricklink@0.0.1-dev"]}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/secret/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "consumer_key"
jsonpath "$.data[0].project" == "alice/bricklink"
jsonpath "$.data[0].version" == "0.0.1-dev"

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/variable/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[*].name" includes "api_base"
jsonpath "$.data[*].name" includes "region"
jsonpath "$.data[*].project" includes "alice/bricklink"

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/secret/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.project" == "alice/bricklink"
jsonpath "$.data.name" == "consumer_key"

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/variable/alice%2Fbricklink.api_base
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.default" == "https://fixture.test/api"
jsonpath "$.data.project" == "alice/bricklink"

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/variable/api_base
Authorization: Bearer {{token}}
HTTP 404

# ---------------------------------------------------------------------------
# Copy carries both. The published target then refuses a write.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/config/version/alice/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "OAuth consumer key"
jsonpath "$.data.required" == true
jsonpath "$.data.version" == "1.0.0"
jsonpath "$.data.default" not exists

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/variable/api_base
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.default" == "https://fixture.test/api"
jsonpath "$.data.version" == "1.0.0"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/variable/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other_secret", "label": "Other"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other_var", "label": "Other", "default": "x"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published"

# The draft is still writable. Deleting its secret does not touch the copy.
DELETE http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200

# ---------------------------------------------------------------------------
# Version delete removes what remains. The dev site pins 0.0.1-dev, and the
# consumer depends on it, so both move to the copy first.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/site/alice/bricklink/dev
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0"}
HTTP 200

PUT http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/bricklink@1.0.0"]}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/secret/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].project" == "alice/bricklink"
jsonpath "$.data[0].version" == "1.0.0"

DELETE http://127.0.0.1:{{port}}/config/version/alice/bricklink/0.0.1-dev
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable/api_base
Authorization: Bearer {{token}}
HTTP 404

POST http://127.0.0.1:{{port}}/config/version/alice/bricklink
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1-dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# The copy still has both.
GET http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/1.0.0/variable/api_base
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.default" == "https://fixture.test/api"
