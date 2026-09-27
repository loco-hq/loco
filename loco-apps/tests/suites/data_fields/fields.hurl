# GET /data/{collection}/fields reads a collection's fields from the version
# the request's site pins. The caller names only the site; the version is
# never in the URL, so re-pinning the site changes the answer.
#
# Fixtures: project testuser/testapp, site "www" pinned to 1-dev.
#   1-dev — items: status (options open/closed), title "Title"; fieldset
#           order status, title. notes: body. public_read grants read on items.
#   2-dev — items: title "Name", priority (integer); fieldset order title,
#           priority. No notes. public_read grants read on items.
#   Both manifests assign public_read to public.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "testuser", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# Authenticated, but no role on testuser/testapp.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "stranger", "password": "password"}
HTTP 200
[Captures]
stranger_token: jsonpath "$.data.token"

# --- A member gets the pinned version's fields: fieldset order, options ---

GET http://127.0.0.1:{{port}}/data/items/fields
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.data[0].name" == "status"
jsonpath "$.data[0].type" == "string"
jsonpath "$.data[0].options" count == 2
jsonpath "$.data[0].options[0].value" == "open"
jsonpath "$.data[0].options[1].label" == "Closed"
jsonpath "$.data[1].name" == "title"
jsonpath "$.data[1].label" == "Title"

# Same answer, same shape, as the version-addressed /schema read.
GET http://127.0.0.1:{{port}}/schema/testuser/testapp/1-dev/field/items/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].name" == "status"
jsonpath "$.data[0].options" count == 2
jsonpath "$.data[1].name" == "title"

# A member reads a collection with no public grant.
GET http://127.0.0.1:{{port}}/data/notes/fields
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "body"

# --- public with a read grant gets the same fields ---

GET http://127.0.0.1:{{port}}/data/items/fields
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 2
jsonpath "$.data[0].name" == "status"
jsonpath "$.data[0].options" count == 2

# The grant covers anyone, so an authenticated non-member reads it too — the
# same rule as /data/items/list.
GET http://127.0.0.1:{{port}}/data/items/fields
Authorization: Bearer {{stranger_token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

# --- public without a read grant is refused ---

GET http://127.0.0.1:{{port}}/data/notes/fields
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 403
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/data/notes/fields
Authorization: Bearer {{stranger_token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 403

# A collection the pinned version does not have is a 404, as for records.
GET http://127.0.0.1:{{port}}/data/nope/fields
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown collection"

# Site headers are required: without them there is no version to read.
GET http://127.0.0.1:{{port}}/data/items/fields
Authorization: Bearer {{token}}
HTTP 400

# --- Re-pinning the site changes the answer ---

PUT http://127.0.0.1:{{port}}/config/site/testuser/testapp/www
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "2-dev"}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "2-dev"

GET http://127.0.0.1:{{port}}/data/items/fields
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].name" == "title"
jsonpath "$.data[0].label" == "Name"
jsonpath "$.data[1].name" == "priority"
jsonpath "$.data[1].type" == "integer"

GET http://127.0.0.1:{{port}}/data/items/fields
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data[0].label" == "Name"
jsonpath "$.data[1].name" == "priority"

# 2-dev has no notes collection.
GET http://127.0.0.1:{{port}}/data/notes/fields
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 404

# And back: pinning 1-dev again serves 1-dev's fields.
PUT http://127.0.0.1:{{port}}/config/site/testuser/testapp/www
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1-dev"}
HTTP 200

GET http://127.0.0.1:{{port}}/data/items/fields
X-Project-Id: testuser/testapp
X-Site-Id: www
HTTP 200
[Asserts]
jsonpath "$.data[0].name" == "status"
jsonpath "$.data[1].label" == "Title"
