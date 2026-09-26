# Copying a version's metadata into a new version id (issue #10).
#
# This is the publish primitive: `0.0.1-dev` -> `0.0.1` snapshots a draft
# into a published, read-only version. Verifies that collections, fields,
# fieldsets, permission sets, and the manifest (dependencies *and* the
# public permission-set assignment) all come across, that the published
# target refuses `/schema` writes, and that the copy's guard rails hold.
#
# Fixture: alice/pub ships one draft version `0.0.1-dev` with a collection,
# two fields, an auto_add fieldset, and a permission set. Deliberately a
# different project from alice/lab so this file and version_lifecycle.hurl
# (which asserts alice/lab starts with no versions) do not depend on the
# order hurl runs them in.

# Login as alice (global). She is an implicit developer on alice/*.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. The draft is there, with everything the copy has to carry.
GET http://127.0.0.1:{{port}}/config/version/alice/pub/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0][1].version" == "0.0.1-dev"

# 2. Copy the draft into a published version id.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.project" == "alice/pub"
jsonpath "$.data.version" == "0.0.1"
jsonpath "$.data.dependencies" count == 1
jsonpath "$.data.dependencies[0]" == "other/proj@v1"
jsonpath "$.data.public_permission_sets" count == 1
jsonpath "$.data.public_permission_sets[0]" == "article_read"

# 3. Both versions now exist; the source is untouched.
GET http://127.0.0.1:{{port}}/config/version/alice/pub/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

# 4. Collections match.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "article"
jsonpath "$.data[0].label" == "Article"
jsonpath "$.data[0].label_plural" == "Articles"
jsonpath "$.data[0].version" == "0.0.1"
jsonpath "$.data[0].project" == "alice/pub"

# 5. Fields match, in auto_add fieldset order.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/field/article/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].name" == "title"
jsonpath "$.data[1].name" == "body"
jsonpath "$.data[0].version" == "0.0.1"

# 6. Fieldsets match — including ordering and the auto_add flag. A copy that
#    went through create_collection would have injected a second, empty
#    `default` set here instead.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/fieldset/article/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "default"
jsonpath "$.data[0].auto_add" == true
jsonpath "$.data[0].fields" count == 2
jsonpath "$.data[0].fields[0]" == "title"
jsonpath "$.data[0].fields[1]" == "body"

# 7. Permission sets match, grants included.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/permission_set/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "article_read"
jsonpath "$.data[0].collections" count == 1
jsonpath "$.data[0].collections[0].collection" == "article"
jsonpath "$.data[0].collections[0].read" == true
jsonpath "$.data[0].collections[0].create" == false

# 8. The published target is read-only to /schema.
PUT http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection/article
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"label": "Edited"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false

POST http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"name": "note", "label": "Note", "label_plural": "Notes"}
HTTP 400

# 9. The source draft is still writable — publishing does not freeze it.
PUT http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/collection/article
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"label": "Article (still editable)"}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Article (still editable)"

# 10. ...and that edit did not leak into the published snapshot.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection/article
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Article"

# 11. Copying onto an existing version conflicts.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false

# 12. Copying from a version that does not exist is a 404, and leaves nothing.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "9.9.9", "from": "nope-dev"}
HTTP 404
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/config/version/alice/pub/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

# 13. A version cannot be copied onto itself.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.1-dev", "from": "0.0.1-dev"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false

# 14. `from` must be a single path segment, like `version`.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.2", "from": "0.0.1/dev"}
HTTP 400

POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.2", "from": ""}
HTTP 400

# 15. Omitting `from` still creates a blank version — the pre-copy behavior.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.2-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.dependencies" count == 0
jsonpath "$.data.public_permission_sets" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.2-dev/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 16. Deleting the published copy leaves the draft intact.
DELETE http://127.0.0.1:{{port}}/config/version/alice/pub/0.0.1
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# 17. A copy that fails part-way rolls back what it wrote, and only that.
#
#     Fixture `0.0.9` holds a fieldset at `article/default` and no manifest,
#     so it is not a version — which is why step 2's manifest pre-check does
#     not catch it and the copy gets as far as writing. Collections and
#     fields copy fine, then the fieldset key collides. The rollback must
#     remove the collection and fields it just created and leave the
#     pre-existing fieldset alone.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0.0.9", "from": "0.0.1-dev"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false

# The collection the copy created is gone again.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# So are the fields, so a retry is not blocked by its own leftovers.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/field/article/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# The fieldset that was already there is untouched — the rollback deletes
# what the copy wrote, not everything under the target version id.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/fieldset/article/default
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Squatter"

# And no manifest was left behind, so `0.0.9` is still not a version.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404
