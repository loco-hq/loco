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
#
# Its manifest depends on other/proj@v1, a fixture too: a copy re-checks the
# dependencies it carries and refuses one whose version does not exist.

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
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0][1].version" == "0.0.1-dev"

# 1b. The draft also ships a bundle: a file tree, not a YAML document. Capture
#     its metadata so the copy can be compared against it. `hash` is a
#     SHA-256 over the tree's contents, so an identical tree hashes alike.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.files" == 2
jsonpath "$.data.size" == 54
[Captures]
draft_bundle_hash: jsonpath "$.data.hash"

# 2. Copy the draft into a published version id.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
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
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

# 4. Collections match.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection/list
Authorization: Bearer {{token}}
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
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "article_read"
jsonpath "$.data[0].collections" count == 1
jsonpath "$.data[0].collections[0].collection" == "article"
jsonpath "$.data[0].collections[0].read" == true
jsonpath "$.data[0].collections[0].create" == false

# 7b. The bundle came across. Metadata is a version directory, not just its
#     YAML — a published snapshot that dropped the frontend would serve the
#     wrong HTML when a site pinned it (issue #37).
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.files" == 2
jsonpath "$.data.size" == 54
# Same bytes in, same hash out.
jsonpath "$.data.hash" == "{{draft_bundle_hash}}"

# 7c. The source still has its tree — a copy, not a move.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.hash" == "{{draft_bundle_hash}}"

# 7d. The copied tree is write-locked with the rest of the published version,
#     so its bytes are safe to cache forever.
#     The body is deliberately not a valid zip: `require_writable` runs before
#     the archive is unpacked, so the error must name the published version
#     rather than complain about the bytes.
PUT http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
```
not a zip
```
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "published"

DELETE http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/bundle
Authorization: Bearer {{token}}
HTTP 400

# 7e. ...and the refused write did not disturb it.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.hash" == "{{draft_bundle_hash}}"

# 8. The published target is read-only to /schema.
PUT http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection/article
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Edited"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false

POST http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "note", "label": "Note", "label_plural": "Notes"}
HTTP 400

# 9. The source draft is still writable — publishing does not freeze it.
PUT http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/collection/article
Authorization: Bearer {{token}}
Content-Type: application/json
{"label": "Article (still editable)"}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Article (still editable)"

# 10. ...and that edit did not leak into the published snapshot.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/collection/article
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Article"

# 11. Copying onto an existing version conflicts.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false

# 12. Copying from a version that does not exist is a 404, and leaves nothing.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "9.9.9", "from": "nope-dev"}
HTTP 404
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/config/version/alice/pub/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2

# 13. A version cannot be copied onto itself.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1-dev", "from": "0.0.1-dev"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false

# 14. `from` must be a single path segment, like `version`.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.2", "from": "0.0.1/dev"}
HTTP 400

POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.2", "from": ""}
HTTP 400

# 15. Omitting `from` still creates a blank version — the pre-copy behavior.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.2-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.dependencies" count == 0
jsonpath "$.data.public_permission_sets" count == 0

GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.2-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# 16. Deleting the published copy leaves the draft intact.
DELETE http://127.0.0.1:{{port}}/config/version/alice/pub/0.0.1
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# ...including the tree the copy created, so a later version reusing that id
# cannot inherit a stale frontend.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1/bundle
Authorization: Bearer {{token}}
HTTP 404

# The draft keeps its own tree.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.1-dev/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.hash" == "{{draft_bundle_hash}}"

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
Content-Type: application/json
{"version": "0.0.9", "from": "0.0.1-dev"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false

# The collection the copy created is gone again.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# So are the fields, so a retry is not blocked by its own leftovers.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/field/article/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# The fieldset that was already there is untouched — the rollback deletes
# what the copy wrote, not everything under the target version id.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/fieldset/article/default
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "Squatter"

# And no manifest was left behind, so `0.0.9` is still not a version.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/manifest
Authorization: Bearer {{token}}
HTTP 404

# 18. Copying a version that has no bundle still succeeds. `0.0.2-dev` was
#     created blank back in step 15, so there is no tree to carry and the
#     result simply has none — not an empty tree, and not a failure.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.2", "from": "0.0.2-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.version" == "0.0.2"

GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.2/bundle
Authorization: Bearer {{token}}
HTTP 404

# 19. The failed copy from step 17 left no tree behind either. File trees are
#     copied after the YAML, so the fieldset collision happened first — but a
#     reordering that wrote the tree before failing would show up here.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.9/bundle
Authorization: Bearer {{token}}
HTTP 404

# 20. A tree already sitting at the target blocks the copy instead of being
#     silently replaced.
#
#     Writing a file tree is a whole-tree replace, so unlike the YAML `create`
#     calls it has no natural "already exists" error. Fixture `0.0.8` holds a
#     bundle and no manifest, which is exactly the case the manifest pre-check
#     cannot see.
POST http://127.0.0.1:{{port}}/config/version/alice/pub
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.8", "from": "0.0.1-dev"}
HTTP 409
[Asserts]
jsonpath "$.ok" == false

# The squatting tree is untouched...
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.8/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.files" == 1
jsonpath "$.data.hash" != "{{draft_bundle_hash}}"

# ...and the refusal came before any YAML was written, so there is nothing to
# roll back and nothing left over.
GET http://127.0.0.1:{{port}}/schema/alice/pub/0.0.8/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0
