# Bundle: a version's static file tree, deployed as a draft /schema write.
#
# Fixture: alice owns alice/testapp (person-owned → implicit developer) with
# three versions — 0-draft (writable), 0-boot (writable, and already has a
# bundle tree on disk so boot has to load one), and 1.0.0 (published).

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. A version with no bundle is a 404, not a boot failure or an empty tree.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "no bundle"

# 2. PUT a dist zip: index.html at the root plus one hashed asset.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.size" == 56
jsonpath "$.data.files" == 2
jsonpath "$.data.hash" matches "^[0-9a-f]{64}$"
jsonpath "$.data.uploaded_at" exists
[Captures]
hash_v1: jsonpath "$.data.hash"

# 3. GET reports metadata for what was stored — not the files.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.hash" == "{{hash_v1}}"
jsonpath "$.data.size" == 56
jsonpath "$.data.files" == 2
jsonpath "$.data.uploaded_at" exists
jsonpath "$.data.index.html" not exists

# 4. A second PUT replaces the whole tree: three files, new hash.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist-v2.zip;
HTTP 200
[Asserts]
jsonpath "$.data.size" == 65
jsonpath "$.data.files" == 3
jsonpath "$.data.hash" != "{{hash_v1}}"
[Captures]
hash_v2: jsonpath "$.data.hash"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.hash" == "{{hash_v2}}"
jsonpath "$.data.files" == 3

# 5. A tree that was on disk before startup is visible — the store loads file
#    trees at boot the same way it loads YAML.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-boot/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.size" == 57
jsonpath "$.data.files" == 2

# 6. A zip that escapes the tree is refused, and the stored tree is untouched.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,traversal.zip;
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unsafe path"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.hash" == "{{hash_v2}}"

# 7. No index.html at the zip root is not a bundle.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,no-index.zip;
HTTP 400
[Asserts]
jsonpath "$.error" contains "index.html"

# 8. Neither is a body that is not a zip at all.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
```
this is not a zip
```
HTTP 400
[Asserts]
jsonpath "$.error" contains "not a readable zip"

# 9. A published version's bytes never change: PUT and DELETE both refuse.
PUT http://127.0.0.1:{{port}}/schema/alice/testapp/1.0.0/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "published and read-only"

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/1.0.0/bundle
Authorization: Bearer {{token}}
HTTP 400
[Asserts]
jsonpath "$.error" contains "published and read-only"

# The published version never received one, so the read is a plain 404.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/1.0.0/bundle
Authorization: Bearer {{token}}
HTTP 404

# 10. An unknown project is a 404 before any of this matters.
PUT http://127.0.0.1:{{port}}/schema/alice/nosuch/0-draft/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 404

# 11. DELETE drops the tree; a second DELETE has nothing to drop.
DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 404

DELETE http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/bundle
Authorization: Bearer {{token}}
HTTP 404
