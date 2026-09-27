# Manifest dependencies (issue #12)
# Verifies: a manifest write refuses a dependency that is not
#           {account}/{project}@{version}, names a version that does not
#           exist, names the manifest's own project, or repeats a project
#           (400, nothing stored). A valid dependency's collections are
#           visible. A version another project depends on cannot be deleted,
#           nor can its project (409, naming the dependents). A version copy
#           carries valid dependencies and refuses bad ones.
#
# alice creates alice/app (the consumer) and alice/lib (the dependency).
# Fixture alice/legacy has a manifest with bad dependencies written before
# they were checked: boot loads it anyway.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "app", "label": "App", "description": "Consumer"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "lib", "label": "Lib", "description": "Dependency"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/lib/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "book", "label": "Book", "label_plural": "Books"}
HTTP 201

# Publish lib 1.0 from the draft.
POST http://127.0.0.1:{{port}}/config/version/alice/lib
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0", "from": "0.0.1-dev"}
HTTP 201

# 1. Malformed entries are refused.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["nope"]}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "nope"

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib"]}
HTTP 400

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@"]}
HTTP 400

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib/extra@1.0"]}
HTTP 400

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["lib@1.0"]}
HTTP 400

# 2. Well-formed but missing: no such project, or no such version of one.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/ghost@0.0.1"]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "alice/ghost@0.0.1"
jsonpath "$.error" contains "does not exist"

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@9.9"]}
HTTP 400

# 3. A version cannot depend on its own project, at any version.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/app@0.0.1-dev"]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "own project"

# 4. Duplicates: the same entry twice, or two versions of one project.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0", "alice/lib@1.0"]}
HTTP 400
[Asserts]
jsonpath "$.error" contains "repeats"

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0", "alice/lib@0.0.1-dev"]}
HTTP 400

# One bad entry refuses the whole write, valid entries included.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0", "nope"]}
HTTP 400

# 5. Nothing refused was stored.
GET http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 0

# 6. A valid dependency is stored, and its collections become visible.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0"]}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 1
jsonpath "$.data.dependencies[0]" == "alice/lib@1.0"

GET http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "book"
jsonpath "$.data[0].project" == "alice/lib"

# A patch without `dependencies` leaves them alone and is not re-checked.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"public_permission_sets": []}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies[0]" == "alice/lib@1.0"

# 7. The dependency cannot be deleted out from under the manifest: not the
#    version, not its project. Both name the dependent version.
DELETE http://127.0.0.1:{{port}}/config/version/alice/lib/1.0
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "alice/app@0.0.1-dev"

DELETE http://127.0.0.1:{{port}}/config/project/alice/lib
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.error" contains "alice/app@0.0.1-dev"

GET http://127.0.0.1:{{port}}/schema/alice/lib/1.0/collection/book
Authorization: Bearer {{token}}
HTTP 200

# A version no one depends on still deletes; the dev site pins lib's draft,
# so make a spare to show the guard is per version.
POST http://127.0.0.1:{{port}}/config/version/alice/lib
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "2.0"}
HTTP 201

DELETE http://127.0.0.1:{{port}}/config/version/alice/lib/2.0
Authorization: Bearer {{token}}
HTTP 200

# 8. Copying app's draft carries its (valid) dependency into 0.0.1, which now
#    depends on lib 1.0 too.
POST http://127.0.0.1:{{port}}/config/version/alice/app
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.dependencies[0]" == "alice/lib@1.0"

# 9. Dropping the draft's dependency is not enough: the published copy still
#    holds it.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": []}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/version/alice/lib/1.0
Authorization: Bearer {{token}}
HTTP 409
[Asserts]
jsonpath "$.error" contains "alice/app@0.0.1"
jsonpath "$.error" not contains "0.0.1-dev"

# Deleting the dependent frees it.
DELETE http://127.0.0.1:{{port}}/config/version/alice/app/0.0.1
Authorization: Bearer {{token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/version/alice/lib/1.0
Authorization: Bearer {{token}}
HTTP 200

# A dependent's own project deletes regardless of what it depends on.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["alice/lib@0.0.1-dev"]}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/project/alice/app
Authorization: Bearer {{token}}
HTTP 200

DELETE http://127.0.0.1:{{port}}/config/project/alice/lib
Authorization: Bearer {{token}}
HTTP 200

# 10. Boot tolerated alice/legacy's bad manifest: it reads back as written,
#     and the version's schema reads with the bad entries skipped.
GET http://127.0.0.1:{{port}}/schema/alice/legacy/0-dev/manifest
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 2
jsonpath "$.data.dependencies[0]" == "nope"

GET http://127.0.0.1:{{port}}/schema/alice/legacy/0-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 11. A copy will not carry them into a new version, and leaves nothing behind.
POST http://127.0.0.1:{{port}}/config/version/alice/legacy
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1", "from": "0-dev"}
HTTP 400
[Asserts]
jsonpath "$.error" contains "nope"

GET http://127.0.0.1:{{port}}/config/version/alice/legacy/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# The fix is the draft's own manifest write, which is checked.
PUT http://127.0.0.1:{{port}}/schema/alice/legacy/0-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": []}
HTTP 200

POST http://127.0.0.1:{{port}}/config/version/alice/legacy
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1", "from": "0-dev"}
HTTP 201
