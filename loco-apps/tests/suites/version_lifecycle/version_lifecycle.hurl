# Version (manifest) lifecycle integration test.
#
# Verifies the full lifecycle for a project version:
#   1. POST /config/version/{user}/{project} creates an empty manifest
#   2. GET /config/version/.../list reflects it
#   3. GET /schema/.../manifest returns dependencies
#   4. PUT /schema/.../manifest updates dependencies
#   5. DELETE /config/version/.../{version} cascades to collections, fields,
#      and fieldsets — a recreated version starts with no stale ordering
#
# Fixture: loco/cards hosts the metadata-editor "cards" site that alice logs
# into; alice/lab is a pre-existing project with no versions yet.

# Login as alice (global). Schema/config writes still send loco/cards headers.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Versions list starts empty for alice/lab
GET http://127.0.0.1:{{port}}/config/version/alice/lab/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0

# 2. Create a draft version
POST http://127.0.0.1:{{port}}/config/version/alice/lab
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0-draft"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.project" == "alice/lab"
jsonpath "$.data.version" == "0-draft"
jsonpath "$.data.dependencies" count == 0

# 3. Empty version body fails
POST http://127.0.0.1:{{port}}/config/version/alice/lab
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": ""}
HTTP 400
[Asserts]
jsonpath "$.ok" == false

# 4. Multi-segment version name fails
POST http://127.0.0.1:{{port}}/config/version/alice/lab
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0/draft"}
HTTP 400

# 5. List now shows the new version
GET http://127.0.0.1:{{port}}/config/version/alice/lab/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0][1].version" == "0-draft"

# 6. GET manifest via the /schema route
GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.project" == "alice/lab"
jsonpath "$.data.version" == "0-draft"
jsonpath "$.data.dependencies" count == 0

# 7. PUT manifest updates dependencies
PUT http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"dependencies": ["other/proj@v1", "more/proj@v2"]}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 2
jsonpath "$.data.dependencies[0]" == "other/proj@v1"
jsonpath "$.data.dependencies[1]" == "more/proj@v2"

# 8. GET reflects updated deps
GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 2

# 9. Add a collection + field at this version (so cascade has something to clean)
POST http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/collection
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"name": "task", "label": "Task", "label_plural": "Tasks"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/field
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"collection": "task", "name": "title", "type": "string"}
HTTP 201

# 10. Sanity: collection + field + default fieldset exist before cascade
GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "task"

GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/field/task/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "title"

GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/fieldset/task/default
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.fields" count == 1
jsonpath "$.data.fields[0]" == "title"

# 11. DELETE cascades the version
DELETE http://127.0.0.1:{{port}}/config/version/alice/lab/0-draft
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 12. Manifest gone
GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

# 13. Collections + fields gone
GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/collection/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 14. Versions list back to empty
GET http://127.0.0.1:{{port}}/config/version/alice/lab/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 15. Re-deleting an absent version errors
DELETE http://127.0.0.1:{{port}}/config/version/alice/lab/0-draft
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

# 16. Recreate the same version id. It must start empty: the deleted version's
#     fieldsets are gone, not waiting to be inherited.
POST http://127.0.0.1:{{port}}/config/version/alice/lab
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"version": "0-draft"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/fieldset/task/list
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 17. Recreating the collection gets a fresh default fieldset, not the old
#     one still ordering a field that no longer exists.
POST http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/collection
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{"name": "task", "label": "Task", "label_plural": "Tasks"}
HTTP 201

GET http://127.0.0.1:{{port}}/schema/alice/lab/0-draft/fieldset/task/default
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.fields" count == 0

# 18. Delete it again; the runner then checks no fieldset YAML is left on
#     disk under alice/lab, so a reload cannot resurrect one.
DELETE http://127.0.0.1:{{port}}/config/version/alice/lab/0-draft
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
