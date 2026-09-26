# Draft-ness is the `-dev` suffix, not any hyphen (issue #49).
#
# A draft accepts `/schema` writes; anything else is published and refuses
# them. `my-app` and `0-draft` contain a hyphen but are published, so they
# refuse writes exactly as `1.0.0` does.
#
# Uses its own project, created here, so it does not disturb what the other
# files in this suite assert about alice/lab and alice/pub.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# Bootstraps 0.0.1-dev, the draft control.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "names", "label": "Names", "description": "Version naming rules"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "my-app"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/version/alice/names
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0-draft"}
HTTP 201

# 1. The -dev draft takes a write.
POST http://127.0.0.1:{{port}}/schema/alice/names/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "task", "label": "Task", "label_plural": "Tasks"}
HTTP 201

# 2. A published version refuses it...
POST http://127.0.0.1:{{port}}/schema/alice/names/1.0.0/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "task", "label": "Task", "label_plural": "Tasks"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "published and read-only"

# 3. ...and so does a hyphenated name without the suffix.
POST http://127.0.0.1:{{port}}/schema/alice/names/my-app/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "task", "label": "Task", "label_plural": "Tasks"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "published and read-only"

POST http://127.0.0.1:{{port}}/schema/alice/names/0-draft/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "task", "label": "Task", "label_plural": "Tasks"}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "published and read-only"

# 4. Nothing was written to the published versions.
GET http://127.0.0.1:{{port}}/schema/alice/names/my-app/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

DELETE http://127.0.0.1:{{port}}/config/project/alice/names
Authorization: Bearer {{token}}
HTTP 200
