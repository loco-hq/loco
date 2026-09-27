# Site pins
# Verifies: a site's version and dataset must exist in its project. Site
#           create and update refuse a dangling pin (400); dataset and version
#           delete refuse while a site pins them (409, naming the sites), and a
#           refused dataset delete leaves its records alone. Project delete
#           still cascades.
#
# No fixtures: alice creates alice/pins, which bootstraps 0.0.1-dev, a dev
# dataset, and a dev site pinning both.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Bootstrap still produces a valid pin.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "pins", "label": "Pins", "description": "Site pin checks"}
HTTP 201

GET http://127.0.0.1:{{port}}/config/site/alice/pins/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

# 2. Create refuses a version or dataset the project does not have.
POST http://127.0.0.1:{{port}}/config/site/alice/pins
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "WWW", "version": "does-not-exist", "dataset": "dev"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "does-not-exist"

POST http://127.0.0.1:{{port}}/config/site/alice/pins
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "WWW", "version": "0.0.1-dev", "dataset": "nope"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "nope"

# An omitted version is the empty string, which names no version.
POST http://127.0.0.1:{{port}}/config/site/alice/pins
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "WWW", "dataset": "dev"}
HTTP 400

GET http://127.0.0.1:{{port}}/config/site/alice/pins/www
Authorization: Bearer {{token}}
HTTP 404

# 3. Update refuses a dangling pin and leaves the site as it was.
PUT http://127.0.0.1:{{port}}/config/site/alice/pins/dev
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "9.9.9"}
HTTP 400

PUT http://127.0.0.1:{{port}}/config/site/alice/pins/dev
Authorization: Bearer {{token}}
Content-Type: application/json
{"dataset": "gone"}
HTTP 400

# A patch that names neither is not re-checked.
PUT http://127.0.0.1:{{port}}/config/site/alice/pins/dev
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Pins Dev"}
HTTP 200

GET http://127.0.0.1:{{port}}/config/site/alice/pins/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Pins Dev"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

# 4. A collection on the draft, then publish it by copy. The copy is a valid
#    pin target.
POST http://127.0.0.1:{{port}}/schema/alice/pins/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "note", "label": "Note", "label_plural": "Notes"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/pins/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "note", "name": "title", "type": "string"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/version/alice/pins
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0", "from": "0.0.1-dev"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/dataset/alice/pins
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "prod", "label": "Prod", "description": "Production"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/site/alice/pins
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "WWW", "version": "1.0.0", "dataset": "prod"}
HTTP 201

POST http://127.0.0.1:{{port}}/data/note/add
Authorization: Bearer {{token}}
X-Project-Id: alice/pins
X-Site-Id: www
Content-Type: application/json
{"title": "keep me"}
HTTP 201

# 5. Deleting a pinned dataset is a 409 naming the site, and its records stay.
DELETE http://127.0.0.1:{{port}}/config/dataset/alice/pins/prod
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "www"

GET http://127.0.0.1:{{port}}/config/dataset/alice/pins/prod
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/data/note/list
Authorization: Bearer {{token}}
X-Project-Id: alice/pins
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# 6. Deleting a pinned version is a 409 naming the site, and nothing of the
#    version is cascaded away.
DELETE http://127.0.0.1:{{port}}/config/version/alice/pins/1.0.0
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.error" contains "www"

GET http://127.0.0.1:{{port}}/schema/alice/pins/1.0.0/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

GET http://127.0.0.1:{{port}}/schema/alice/pins/1.0.0/field/note/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# Two sites on one version: both are named.
PUT http://127.0.0.1:{{port}}/config/site/alice/pins/www
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1-dev"}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/version/alice/pins/0.0.1-dev
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.error" contains "dev"
jsonpath "$.error" contains "www"

# 7. Unpinned, the version goes.
DELETE http://127.0.0.1:{{port}}/config/version/alice/pins/1.0.0
Authorization: Bearer {{token}}
HTTP 200

# Re-pin www off prod; now prod goes, records and all.
PUT http://127.0.0.1:{{port}}/config/site/alice/pins/www
Authorization: Bearer {{token}}
Content-Type: application/json
{"dataset": "dev"}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/dataset/alice/pins/prod
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/dataset/alice/pins/prod
Authorization: Bearer {{token}}
HTTP 404

# A deleted dataset is no longer a pin target.
PUT http://127.0.0.1:{{port}}/config/site/alice/pins/www
Authorization: Bearer {{token}}
Content-Type: application/json
{"dataset": "prod"}
HTTP 400

# 8. Missing still 404s, not 409.
DELETE http://127.0.0.1:{{port}}/config/version/alice/pins/1.0.0
Authorization: Bearer {{token}}
HTTP 404

# 9. Project delete is not blocked by its own pins: it cascades everything.
DELETE http://127.0.0.1:{{port}}/config/project/alice/pins
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/project/alice/pins
Authorization: Bearer {{token}}
HTTP 404
