# Dependency access (issue #86)
# Verifies: a manifest may name only a project its writer can read — any
#           role on it, or owner of its org. A dependency that exists but is
#           not readable gets the same 400, body for body, as one that does
#           not exist. An entry the manifest already names is not re-checked
#           for access, and a version copy carries its source's dependencies
#           whoever copies. Token-less public cannot write a manifest at all.
#
# alice owns alice/lib, alice/app, and the org acme (acme/lib). bob owns
# bob/app and starts with no access to any of alice's or acme's projects.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "bob", "password": "password"}
HTTP 200
[Captures]
bob_token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "app", "label": "Bob app", "description": "Consumer"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "lib", "label": "Lib", "description": "Private dependency"}
HTTP 201

# 1. alice/lib@1.0 does not exist yet: bob's write is refused.
PUT http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0"]}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "does not exist"
[Captures]
missing_body: body

# 2. alice publishes it. It exists now, but bob cannot read alice/lib: the
#    same 400, the same body, so the write does not reveal that it exists.
POST http://127.0.0.1:{{port}}/config/version/alice/lib
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"version": "1.0", "from": "0.0.1-dev"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0"]}
HTTP 400
[Asserts]
body == "{{missing_body}}"

# A version of that project that does not exist reads the same way, and so
# does a project that does not exist.
PUT http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["alice/lib@9.9"]}
HTTP 400
[Asserts]
jsonpath "$.error" == "dependency \"alice/lib@9.9\" names version 9.9 of alice/lib, which does not exist"

PUT http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["alice/ghost@1.0"]}
HTTP 400
[Asserts]
jsonpath "$.error" == "dependency \"alice/ghost@1.0\" names version 1.0 of alice/ghost, which does not exist"

GET http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 0

# 3. A readable dependency is accepted: alice owns her person account.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "app", "label": "Alice app", "description": "Consumer"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0"]}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies[0]" == "alice/lib@1.0"

# 4. An org owner may depend on the org's project.
POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "acme"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "lib", "label": "Acme lib", "description": "Org dependency", "account": "acme"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0", "acme/lib@0.0.1-dev"]}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 2

# bob, not an acme member, may not.
PUT http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["acme/lib@0.0.1-dev"]}
HTTP 400
[Asserts]
jsonpath "$.error" == "dependency \"acme/lib@0.0.1-dev\" names version 0.0.1-dev of acme/lib, which does not exist"

# 5. Any role on the project is read access: as an editor of alice/lib,
#    bob may depend on it.
POST http://127.0.0.1:{{port}}/config/member/alice/lib
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "editor"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/bob/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["alice/lib@1.0"]}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies[0]" == "alice/lib@1.0"

# 6. An entry the manifest already names is not re-checked for access. bob,
#    now a developer of alice/app but still not in acme, can re-send the
#    whole list and edit the rest of it...
POST http://127.0.0.1:{{port}}/config/member/alice/app
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "developer"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["acme/lib@0.0.1-dev", "alice/lib@1.0"], "public_permission_sets": []}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 2

# ...and drop an entry, but not add one they cannot read, nor move an
# existing one to another version: that names something new.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["acme/lib@0.0.1-dev"]}
HTTP 200

POST http://127.0.0.1:{{port}}/config/version/acme/lib
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"version": "1.0", "from": "0.0.1-dev"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"dependencies": ["acme/lib@1.0"]}
HTTP 400
[Asserts]
jsonpath "$.error" == "dependency \"acme/lib@1.0\" names version 1.0 of acme/lib, which does not exist"

GET http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{bob_token}}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies" count == 1
jsonpath "$.data.dependencies[0]" == "acme/lib@0.0.1-dev"

# 7. A version copy is a snapshot: bob publishes alice/app's draft, and the
#    copy carries acme/lib though bob cannot read it.
POST http://127.0.0.1:{{port}}/config/version/alice/app
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.dependencies[0]" == "acme/lib@0.0.1-dev"

# 8. Token-less public cannot write a manifest, whatever it names.
PUT http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Content-Type: application/json
{"dependencies": []}
HTTP 401

GET http://127.0.0.1:{{port}}/schema/alice/app/0.0.1-dev/manifest
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data.dependencies[0]" == "acme/lib@0.0.1-dev"
