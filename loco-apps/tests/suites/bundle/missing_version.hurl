# A /schema request to a version that does not exist — no manifest, so never
# created through /config — is a 404, read or write, and writes nothing (#95).
# The runner then checks that no `9.9.9-dev` directory appeared on disk.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Every kind of write is refused before it reaches the store.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "tasks", "label": "Task", "label_plural": "Tasks"}
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "unknown version: alice/testapp@9.9.9-dev"

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown version: alice/testapp@9.9.9-dev"

POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "tasks", "name": "title", "type": "string"}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/fieldset
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "tasks", "name": "summary", "fields": []}
HTTP 404

POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "public_tasks", "collections": []}
HTTP 404

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": []}
HTTP 404

# 2. The 404 comes before the body is read: a body that would be a 400 on a
#    real draft is still a 404 here.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"not": "a collection"}
HTTP 404

# 3. A published name that does not exist is a 404 too, not "published and
#    read-only": the version is missing before it is anything else.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "tasks"}
HTTP 404

# 4. Reads of a missing version are 404s as well, not an empty version.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/collection/list
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown version: alice/testapp@9.9.9-dev"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/manifest
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.9-dev/bundle
Authorization: Bearer {{token}}
HTTP 404

# 5. Nothing was created: the version is still not in the project's list.
GET http://127.0.0.1:{{port}}/config/version/alice/testapp/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[*][1].version" not includes "9.9.9-dev"

# 6. Once /config creates it, the same write goes through.
POST http://127.0.0.1:{{port}}/config/version/alice/testapp
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "9.9.8-dev"}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.8-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "tasks", "label": "Task", "label_plural": "Tasks"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/9.9.8-dev/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
