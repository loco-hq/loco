# A site can only reach collections in the version it pins.
#
# Fixtures: project testuser/testapp with two versions.
#   0-draft — collection "tasks", and permission set "public_read" granting
#             public read on BOTH "tasks" and "secrets"
#   1-draft — collection "secrets" (exists nowhere else)
#   site "pinned-old" pins 0-draft and assigns public_read to public
#   site "pinned-new" pins 1-draft
#
# Before this was fixed, /data checked collection existence with a prefix scan
# of {user}/{project}/ over the global SchemaStore, which matches every version
# of the project. "secrets" therefore resolved on the site pinned to 0-draft,
# and the permission set granting its name made that an unauthenticated read of
# a collection the site does not pin.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "testuser", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# --- The collection in the pinned version is reachable ---

GET http://127.0.0.1:{{port}}/data/tasks/list
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# --- A collection from another version of the same project is not ---

GET http://127.0.0.1:{{port}}/data/secrets/list
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" contains "unknown collection"

# Writes are gated by the same resolution, not just reads.
POST http://127.0.0.1:{{port}}/data/secrets/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
Content-Type: application/json
{"value": "hunter2"}
HTTP 404

# --- The security case: a public grant cannot conjure the collection ---
#
# public_read grants read on "secrets" and the site assigns it to public, so
# authorization alone would allow this. It is 404 because the site pins
# 0-draft, where no such collection exists.

GET http://127.0.0.1:{{port}}/data/secrets/list
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
HTTP 404
[Asserts]
jsonpath "$.ok" == false

# The same grant on a collection the site does pin still works, so the 404
# above is about version pinning and not a broken permission set.

GET http://127.0.0.1:{{port}}/data/tasks/list
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# --- "secrets" really does exist — a site pinned to 1-draft reaches it ---
#
# Without this the 404s above would also pass if the fixture were simply
# missing.

POST http://127.0.0.1:{{port}}/data/secrets/add
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-new
Content-Type: application/json
{"value": "hunter2"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.fields.value" == "hunter2"

GET http://127.0.0.1:{{port}}/data/secrets/list
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-new
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# --- Records written under one collection do not leak into the other ---
#
# Both sites share the "dev" dataset, so isolation here is the lake key doing
# its job: the owner-qualified key keeps "tasks" and "secrets" apart.

GET http://127.0.0.1:{{port}}/data/tasks/list
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# --- A dependency's collection is not reachable by its bare name ---
#
# 0-draft depends on pkg/contacts@1.0, which owns "leads". /schema resolves
# dep-owned metadata by bare name today, but /data deliberately does not: a
# bare name means this project. Naming a dependency's collection needs the
# qualified form, which is #28. This asserts the deferral is real rather than
# accidental — when #28 lands, this expectation changes to a 200 on the
# qualified name and a 404 on the bare one.

GET http://127.0.0.1:{{port}}/data/leads/list
Authorization: Bearer {{token}}
X-Project-Id: testuser/testapp
X-Site-Id: pinned-old
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown collection"

# The dependency is genuinely installed and visible to /schema — so the 404
# above is the resolution rule, not a missing or unlinked fixture.

GET http://127.0.0.1:{{port}}/schema/testuser/testapp/0-draft/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data[*].name" includes "leads"
