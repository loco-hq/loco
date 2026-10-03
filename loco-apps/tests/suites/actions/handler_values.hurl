# Handlers read the installer's values and call out over HTTP (issue #102).
#
# alice/pkg declares the secrets and variables and owns the pull handler.
# alice/store installs that package and sets the values on its own dev
# dataset. The handler reads those rows, the package's variable defaults
# when the store set none, and nothing the store declares under the same
# name. A value stored on the package's own dataset is a different dataset.
# upstream's default points nowhere; the store sets it to the fixture
# server the runner started ({{upstream}}).
#
# Same server as actions.hurl. These projects are not alice/fixture,
# alice/shop, or alice/other.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "pkg", "label": "Package", "description": "Owns pull"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "token", "label": "Token", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "upstream",
  "label": "Upstream",
  "required": true,
  "default": "http://127.0.0.1:9/?k=1"
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "label",
  "label": "Label",
  "required": false,
  "default": "from-the-package-default"
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/variable
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "region", "label": "Region", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pkg/0.0.1-dev/action
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "pull",
  "label": "Pull",
  "description": "Reads the installer's token, label, region, and upstream.",
  "params": [
    {"name": "call", "type": "boolean", "label": "Call"},
    {"name": "path", "type": "string", "label": "Path"}
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "store", "label": "Store", "description": "Installs pkg"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/pkg@0.0.1-dev"]}
HTTP 200

# The store's own secret of the same name, and one the package never declares.
POST http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "token", "label": "Store token", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/store/0.0.1-dev/secret
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "store_only", "label": "Store only"}
HTTP 201

# A value on the package's own dataset. The store's site must not see it.
PUT http://127.0.0.1:{{port}}/config/secret/alice/pkg/dev/token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "from-the-package-dataset"}
HTTP 200
[Asserts]
jsonpath "$.data.set" == true
body not contains "from-the-package-dataset"

# ---------------------------------------------------------------------------
# Missing required config is 400 and the handler does not run. upstream's
# default covers that variable, so only token and region are named.
# ---------------------------------------------------------------------------

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.pull
Authorization: Bearer {{token}}
X-Project-Id: alice/store
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.data" not exists
jsonpath "$.diagnostics" count == 2
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "token"
jsonpath "$.diagnostics[0].message" == "secret 'token' is not set"
jsonpath "$.diagnostics[1].kind" == "required"
jsonpath "$.diagnostics[1].path" == "region"
jsonpath "$.diagnostics[1].message" == "variable 'region' is not set"
body not contains "ran"
body not contains "from-the-package-dataset"

# The store's own token does not satisfy the package's required secret.
PUT http://127.0.0.1:{{port}}/config/variable/alice/store/dev/alice%2Fpkg.region
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "eu"}
HTTP 200

PUT http://127.0.0.1:{{port}}/config/secret/alice/store/dev/token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "store-own-token"}
HTTP 200
[Asserts]
body not contains "store-own-token"

PUT http://127.0.0.1:{{port}}/config/secret/alice/store/dev/store_only
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "store-only-token"}
HTTP 200
[Asserts]
body not contains "store-only-token"

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.pull
Authorization: Bearer {{token}}
X-Project-Id: alice/store
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 400
[Asserts]
jsonpath "$.error" == "required configuration is not set"
jsonpath "$.data" not exists
jsonpath "$.diagnostics" count == 1
jsonpath "$.diagnostics[0].kind" == "required"
jsonpath "$.diagnostics[0].path" == "token"
jsonpath "$.diagnostics[0].message" == "secret 'token' is not set"
body not contains "ran"
body not contains "store-own-token"
body not contains "store-only-token"
body not contains "from-the-package-dataset"

# The package's own site reads the package dataset, including that token.
PUT http://127.0.0.1:{{port}}/config/variable/alice/pkg/dev/region
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "pkg-region"}
HTTP 200

POST http://127.0.0.1:{{port}}/actions/pull
Authorization: Bearer {{token}}
X-Project-Id: alice/pkg
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.ran" == true
jsonpath "$.data.dataset_id" == "alice/pkg/dev"
jsonpath "$.data.token" == "from-the-package-dataset"
jsonpath "$.data.region" == "pkg-region"
jsonpath "$.data.upstream" == "http://127.0.0.1:9/?k=1"
jsonpath "$.data.label" == "from-the-package-default"
jsonpath "$.data.store_only.error" contains "not declared"
jsonpath "$.data.http_body" not exists
body not contains "store-own-token"
body not contains "store-only-token"
body not contains "installer-token"

# ---------------------------------------------------------------------------
# The store's site reads the store's rows for the package's declarations.
# label and upstream are still the package defaults. store_only is not a
# package declaration, so its value is not returned.
# ---------------------------------------------------------------------------

PUT http://127.0.0.1:{{port}}/config/secret/alice/store/dev/alice%2Fpkg.token
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "installer-token"}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "token"
jsonpath "$.data.project" == "alice/pkg"
jsonpath "$.data.set" == true
body not contains "installer-token"

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.pull
Authorization: Bearer {{token}}
X-Project-Id: alice/store
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.ran" == true
jsonpath "$.data.dataset_id" == "alice/store/dev"
jsonpath "$.data.token" == "installer-token"
jsonpath "$.data.region" == "eu"
jsonpath "$.data.label" == "from-the-package-default"
jsonpath "$.data.upstream" == "http://127.0.0.1:9/?k=1"
jsonpath "$.data.store_only.error" contains "not declared"
jsonpath "$.data.http_body" not exists
body not contains "from-the-package-dataset"
body not contains "store-own-token"
body not contains "store-only-token"

# Nothing listens on the default. With no path the fixture appends /ok,
# so the call is http://127.0.0.1:9/?k=1/ok. A 502 must not echo it.
POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.pull
Authorization: Bearer {{token}}
X-Project-Id: alice/store
X-Site-Id: dev
Content-Type: application/json
{"input": {"call": true}}
HTTP 502
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" startsWith "upstream 0:"
jsonpath "$.data" not exists
body not contains "127.0.0.1:9"
body not contains "k=1"

# The store points upstream at the fixture server, then the handler calls it.
PUT http://127.0.0.1:{{port}}/config/variable/alice/store/dev/alice%2Fpkg.upstream
Authorization: Bearer {{token}}
Content-Type: application/json
{"value": "{{upstream}}"}
HTTP 200
[Asserts]
jsonpath "$.data.value" == "{{upstream}}"
jsonpath "$.data.source" == "value"

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.pull
Authorization: Bearer {{token}}
X-Project-Id: alice/store
X-Site-Id: dev
Content-Type: application/json
{"input": {"call": true, "path": "/ok"}}
HTTP 200
[Asserts]
jsonpath "$.data.ran" == true
jsonpath "$.data.http_body" == "upstream ok"
jsonpath "$.data.upstream" == "{{upstream}}"
jsonpath "$.data.token" == "installer-token"
jsonpath "$.data.label" == "from-the-package-default"
body not contains "from-the-package-dataset"
body not contains "store-own-token"

POST http://127.0.0.1:{{port}}/actions/alice%2Fpkg.pull
Authorization: Bearer {{token}}
X-Project-Id: alice/store
X-Site-Id: dev
Content-Type: application/json
{"input": {"call": true, "path": "/fail"}}
HTTP 502
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "upstream 503: upstream is down"
jsonpath "$.data" not exists
body not contains "installer-token"
