# Names /config creates are held to a charset (issue #82).
#
# Project, dataset, and site: [a-z0-9_], starting with a letter or _.
# Version: [a-z0-9._-], not starting with '.'. Both at most 63 characters.
# A `.loco-` name would be skipped by `load_all` as a temp artefact and vanish
# on the next boot; a version with '@' could never be named as a dependency.
#
# Write-only: alice/legacy-app in the fixtures predates the rule (version
# `1.0-Old`, dataset `old-data`, site `old-site`) and still loads.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# --- project ---

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": ".loco-x", "label": "X", "description": ""}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "project name \".loco-x\" must be"
jsonpath "$.error" contains "a-z, 0-9, and _"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "my-app", "label": "X", "description": ""}
HTTP 400

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "Crm", "label": "X", "description": ""}
HTTP 400

GET http://127.0.0.1:{{port}}/config/project/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[*].id" not contains "alice/.loco-x"
jsonpath "$.data[*].id" not contains "alice/my-app"

# A valid name bootstraps 0.0.1-dev, dataset dev, and site dev — the
# bootstrap's own names pass the rule.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "names", "label": "Names", "description": "Name charset"}
HTTP 201

GET http://127.0.0.1:{{port}}/config/site/alice/names/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

# --- version ---

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": ".hidden"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "version \".hidden\" must be"
jsonpath "$.error" contains "not starting with '.'"

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": ".."}
HTTP 400

# '@' separates project from version in a manifest dependency.
POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0@beta"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "version \"1.0@beta\" must be"

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "V1"}
HTTP 400

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0 final"}
HTTP 400

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1234567890123456789012345678901234567890123456789012345678901234"}
HTTP 400

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "my-app"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.2-dev"}
HTTP 201

# A copy target is a new version too.
POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": ".loco-1", "from": "0.0.1-dev"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "version \".loco-1\" must be"

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0@beta", "from": "0.0.1-dev"}
HTTP 400

# 0.0.1-dev, 0.0.1, my-app, 0.0.2-dev — none of the rejected ones.
GET http://127.0.0.1:{{port}}/config/version/alice/names/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 4

# --- dataset ---

POST http://127.0.0.1:{{port}}/config/dataset/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": ".loco-d", "label": "D", "description": ""}
HTTP 400
[Asserts]
jsonpath "$.error" contains "dataset name \".loco-d\" must be"

POST http://127.0.0.1:{{port}}/config/dataset/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "old-data", "label": "D", "description": ""}
HTTP 400

POST http://127.0.0.1:{{port}}/config/dataset/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "prod", "label": "Prod", "description": ""}
HTTP 201

# --- site ---

POST http://127.0.0.1:{{port}}/config/site/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": ".loco-s", "label": "S", "version": "0.0.1", "dataset": "prod"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "site name \".loco-s\" must be"

POST http://127.0.0.1:{{port}}/config/site/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www.x", "label": "S", "version": "0.0.1", "dataset": "prod"}
HTTP 400

POST http://127.0.0.1:{{port}}/config/site/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "WWW", "version": "0.0.1", "dataset": "prod"}
HTTP 201

GET http://127.0.0.1:{{port}}/config/dataset/alice/names/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

GET http://127.0.0.1:{{port}}/config/site/alice/names/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

DELETE http://127.0.0.1:{{port}}/config/project/alice/names
Authorization: Bearer {{token}}
HTTP 200

# --- names already on disk load, and can be read and copied from ---

GET http://127.0.0.1:{{port}}/config/project/alice/legacy-app
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/site/alice/legacy-app/old-site
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "1.0-Old"
jsonpath "$.data.dataset" == "old-data"

GET http://127.0.0.1:{{port}}/config/dataset/alice/legacy-app/old-data
Authorization: Bearer {{token}}
HTTP 200

# `from` names a version that exists; it is not held to the charset.
POST http://127.0.0.1:{{port}}/config/version/alice/legacy-app
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.1", "from": "1.0-Old"}
HTTP 201
