# Secret and variable values per dataset (issue #101).
#
# Values are /config, not /schema. A secret list says whether the value is
# set and never returns it. A variable list returns the stored value, or the
# declaration's default when nothing is set. Writes are developer; an editor
# can read both lists. A list is what the versions this dataset's sites pin
# declare. A write is allowed when any version of the project declares the
# name, itself or through a direct dependency.
#
# alice/bricklink is the package. alice/shop is the consumer. Each project
# bootstraps 0.0.1-dev, a dev dataset, and a dev site.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "bob", "password": "password"}
HTTP 200
[Captures]
bob: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "bricklink", "label": "BrickLink", "description": "Package"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "consumer_key", "label": "Consumer key", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "api_base",
  "label": "API base",
  "default": "https://api.bricklink.test/v1"
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "region", "label": "Region"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Consumer"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/bricklink@0.0.1-dev"]}
HTTP 200

POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "label_prefix", "label": "Label prefix", "default": "Lot"}
HTTP 201

# `$` is not a collection name, so the reserved lake collections cannot be
# declared or read. Ciphertext lives under the literal collection `$secrets`,
# which `/data` never resolves to.
POST http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "$secrets", "label": "no"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "$"

GET http://127.0.0.1:{{port}}/data/$secrets/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown collection"

GET http://127.0.0.1:{{port}}/data/$variables/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown collection"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
Content-Type: application/json
{"queries": {"reserved": {"collection": "$secrets"}}}
HTTP 200
[Asserts]
jsonpath "$.data.reserved.diagnostics[0].kind" == "unknown_collection"

# ---------------------------------------------------------------------------
# Auth. No token is 401. A non-member is 403. An editor can read, not write.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
HTTP 401

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
HTTP 401

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{bob}}
HTTP 403

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{bob}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 403
[Asserts]
body not contains "s3cret-value-one"

POST http://127.0.0.1:{{port}}/config/member/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{bob}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].set" == false

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{bob}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 403
[Asserts]
body not contains "s3cret-value-one"

DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{bob}}
HTTP 403

# ---------------------------------------------------------------------------
# Secrets. The list is the pinned version's declarations. A bare name is
# shop's own secret, which it did not declare. The qualified name is the
# dependency's.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "consumer_key"
jsonpath "$.data[0].project" == "alice/bricklink"
jsonpath "$.data[0].set" == false
jsonpath "$.data[0].updated_at" == null
jsonpath "$.data[0].value" not exists

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/nope
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "not declared"
body not contains "s3cret-value-one"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/label_prefix
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "not declared"
body not contains "s3cret-value-one"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "not declared"
body not contains "s3cret-value-one"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "consumer_key"
jsonpath "$.data.project" == "alice/bricklink"
jsonpath "$.data.set" == true
jsonpath "$.data.updated_at" matches "^[0-9]{4}-"
jsonpath "$.data.value" not exists
body not contains "s3cret-value-one"

GET http://127.0.0.1:{{port}}/schema/alice/bricklink/0.0.1-dev/secret/consumer_key
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.value" not exists
body not contains "s3cret-value-one"

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == true
jsonpath "$.data[0].value" not exists
body not contains "s3cret-value-one"

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "s3cret-value-two"}
HTTP 200
[Asserts]
jsonpath "$.data.set" == true
jsonpath "$.data.value" not exists
body not contains "s3cret-value-one"
body not contains "s3cret-value-two"

# An editor still cannot overwrite or delete a value that is set.
PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{bob}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 403
[Asserts]
body not contains "s3cret-value-one"
body not contains "s3cret-value-two"

DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{bob}}
HTTP 403

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{bob}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == true
body not contains "s3cret-value-two"

# ---------------------------------------------------------------------------
# List vs write. `other` is pinned at an empty version, so its list is empty,
# but the write is allowed because 0.0.1-dev declares the secret. Re-pinning
# the site makes the stored value show up. dev's value is a different row.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/config/version/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "bare"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/dataset/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other", "description": ""}
HTTP 201

POST http://127.0.0.1:{{port}}/config/site/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other", "version": "bare", "dataset": "other"}
HTTP 201

PUT http://127.0.0.1:{{port}}/config/secret/alice/shop/other/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "s3cret-value-one"}
HTTP 200
[Asserts]
jsonpath "$.data.set" == true
body not contains "s3cret-value-one"

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/other/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0
body not contains "s3cret-value-one"

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == true
body not contains "s3cret-value-two"

PUT http://127.0.0.1:{{port}}/config/site/alice/shop/other
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1-dev"}
HTTP 200

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/other/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "consumer_key"
jsonpath "$.data[0].project" == "alice/bricklink"
jsonpath "$.data[0].set" == true
body not contains "s3cret-value-one"

DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/other/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/other/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == false

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == true
body not contains "s3cret-value-two"

# A pinned dataset cannot be deleted. Unpin it, delete it, and the value is
# gone: a new dataset of the same name, pinned at the declaring version,
# lists the secret unset.
DELETE http://127.0.0.1:{{port}}/config/dataset/alice/shop/other
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.error" contains "other"

DELETE http://127.0.0.1:{{port}}/config/site/alice/shop/other
Authorization: Bearer {{token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/dataset/alice/shop/other
Authorization: Bearer {{token}}
HTTP 200

POST http://127.0.0.1:{{port}}/config/dataset/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other", "description": ""}
HTTP 201

POST http://127.0.0.1:{{port}}/config/site/alice/shop
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "other", "label": "Other", "version": "0.0.1-dev", "dataset": "other"}
HTTP 201

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/other/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == false
body not contains "s3cret-value-one"

DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
body not contains "s3cret-value-one"
body not contains "s3cret-value-two"

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == false
jsonpath "$.data[0].updated_at" == null
body not contains "s3cret-value-two"

DELETE http://127.0.0.1:{{port}}/config/secret/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "secret value not found"

GET http://127.0.0.1:{{port}}/config/secret/alice/shop/missing/list
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "dataset not found"

# ---------------------------------------------------------------------------
# Variables. Same declarations, but the value comes back. An unset variable
# with a default lists that default. An editor can read and cannot write.
# ---------------------------------------------------------------------------

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].name" == "api_base"
jsonpath "$.data[0].project" == "alice/bricklink"
jsonpath "$.data[0].set" == false
jsonpath "$.data[0].source" == "default"
jsonpath "$.data[0].value" == "https://api.bricklink.test/v1"
jsonpath "$.data[0].updated_at" == null
jsonpath "$.data[1].name" == "region"
jsonpath "$.data[1].project" == "alice/bricklink"
jsonpath "$.data[1].set" == false
jsonpath "$.data[1].value" == null
jsonpath "$.data[1].source" not exists
jsonpath "$.data[2].name" == "label_prefix"
jsonpath "$.data[2].project" == "alice/shop"
jsonpath "$.data[2].set" == false
jsonpath "$.data[2].source" == "default"
jsonpath "$.data[2].value" == "Lot"

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{bob}}
HTTP 200
[Asserts]
jsonpath "$.data[0].value" == "https://api.bricklink.test/v1"
jsonpath "$.data[2].value" == "Lot"

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/alice%2Fbricklink.api_base
Authorization: Bearer {{bob}}
Content-Type: application/json
{"value": "https://store.example/api"}
HTTP 403

DELETE http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/alice%2Fbricklink.api_base
Authorization: Bearer {{bob}}
HTTP 403

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/alice%2Fbricklink.api_base
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "https://store.example/api"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "api_base"
jsonpath "$.data.project" == "alice/bricklink"
jsonpath "$.data.set" == true
jsonpath "$.data.source" == "value"
jsonpath "$.data.value" == "https://store.example/api"
jsonpath "$.data.updated_at" matches "^[0-9]{4}-"

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{bob}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == true
jsonpath "$.data[0].source" == "value"
jsonpath "$.data[0].value" == "https://store.example/api"

DELETE http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/alice%2Fbricklink.api_base
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].set" == false
jsonpath "$.data[0].source" == "default"
jsonpath "$.data[0].value" == "https://api.bricklink.test/v1"
jsonpath "$.data[0].updated_at" == null

# An empty string is a stored value, distinct from unset.
PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/label_prefix
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": ""}
HTTP 200
[Asserts]
jsonpath "$.data.set" == true
jsonpath "$.data.source" == "value"
jsonpath "$.data.value" == ""

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[2].set" == true
jsonpath "$.data[2].value" == ""
jsonpath "$.data[2].source" == "value"

DELETE http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/label_prefix
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[2].set" == false
jsonpath "$.data[2].source" == "default"
jsonpath "$.data[2].value" == "Lot"

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/nope
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "x"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "not declared"

PUT http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/alice%2Fbricklink.consumer_key
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "x"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "not declared"

DELETE http://127.0.0.1:{{port}}/config/variable/alice/shop/dev/alice%2Fbricklink.api_base
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "variable value not found"

GET http://127.0.0.1:{{port}}/config/variable/alice/shop/missing/list
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "dataset not found"
