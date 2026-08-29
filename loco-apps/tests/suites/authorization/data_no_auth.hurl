# Public is a policy, not a hole. Grants live on permission sets, stacked
# by union, assigned to `public` on the pinned **version's manifest** — not
# on the site. A site is a URL pointing at (version, dataset).
#
# Fixtures: alice/testapp has two snapshots that differ only in policy, and
# a site pinning each. Different policy is a different version.
#   0-draft  manifest public_permission_sets: [guestbook_read, guestbook_create]
#   1-draft  manifest public_permission_sets: [secrets_read, wiki_public]
#   site `dev`  → 0-draft   site `open` → 1-draft   (both on dataset `dev`)
#
# Both versions declare all four collections and all four sets, so every
# difference below is the assignment and nothing else:
#   widgets   — never assigned
#   guestbook — guestbook_read + guestbook_create stacked, on 0-draft only
#   secrets   — secrets_read, on 1-draft only
#   wiki      — wiki_public (all four verbs), on 1-draft only
#
# Token-less requests are principal `public`. Each verb is whatever the
# assigned sets grant; unspecified flags are false.

# 1. Private collection — no anonymous list/add
GET http://127.0.0.1:{{port}}/data/widgets/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 403
[Asserts]
jsonpath "$.ok" == false

POST http://127.0.0.1:{{port}}/data/widgets/add
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"size": "large"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 2. Stacked sets (read + create) — list/add/get work
GET http://127.0.0.1:{{port}}/data/guestbook/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0

POST http://127.0.0.1:{{port}}/data/guestbook/add
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"message": "hello from public"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.owner" == "public"
jsonpath "$.data.created_by" == "public"
[Captures]
record_id: jsonpath "$.data.id"

GET http://127.0.0.1:{{port}}/data/guestbook/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1

GET http://127.0.0.1:{{port}}/data/guestbook/get/{{record_id}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.id" == {{record_id}}

# 3. create does not imply update or delete
PUT http://127.0.0.1:{{port}}/data/guestbook/update/{{record_id}}
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"message": "edited"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

DELETE http://127.0.0.1:{{port}}/data/guestbook/delete/{{record_id}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 403
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/data/guestbook/get/{{record_id}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 4. Authenticated non-member cannot use the public-create hole.
# `carol` is never added to alice/testapp (unlike bob, who authorization.hurl
# promotes on this shared in-memory server).
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "carol", "password": "password"}
HTTP 200
[Captures]
carol_token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/data/guestbook/add
Authorization: Bearer {{carol_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"message": "carol as herself"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Carol *can* list a collection a public set grants read on
GET http://127.0.0.1:{{port}}/data/guestbook/list
Authorization: Bearer {{carol_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 5. Assignment is per version: `secrets_read` is assigned by 1-draft, which
#    `open` pins, and not by 0-draft, which `dev` pins. Both versions define
#    the collection and the set — only the manifest assignment differs.
#    create is not granted by either.
GET http://127.0.0.1:{{port}}/data/secrets/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 403

GET http://127.0.0.1:{{port}}/data/secrets/list
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0

POST http://127.0.0.1:{{port}}/data/secrets/add
X-Project-Id: alice/testapp
X-Site-Id: open
Content-Type: application/json
{"note": "nope"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Guestbook grants on 0-draft do not follow the visitor to a site pinning
# 1-draft, which defines the same collection and set but assigns neither.
GET http://127.0.0.1:{{port}}/data/guestbook/list
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 403

# 6. Project developer can still mutate a private collection
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/data/secrets/add
Authorization: Bearer {{alice_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.owner" == "alice"
[Captures]
secret_id: jsonpath "$.data.id"

DELETE http://127.0.0.1:{{port}}/data/secrets/delete/{{secret_id}}
Authorization: Bearer {{alice_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 7. Public CRUD when the set grants all four verbs (wiki on `open`).
POST http://127.0.0.1:{{port}}/data/wiki/add
X-Project-Id: alice/testapp
X-Site-Id: open
Content-Type: application/json
{}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.owner" == "public"
[Captures]
wiki_id: jsonpath "$.data.id"

PUT http://127.0.0.1:{{port}}/data/wiki/update/{{wiki_id}}
X-Project-Id: alice/testapp
X-Site-Id: open
Content-Type: application/json
{}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

GET http://127.0.0.1:{{port}}/data/wiki/get/{{wiki_id}}
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true

DELETE http://127.0.0.1:{{port}}/data/wiki/delete/{{wiki_id}}
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true

GET http://127.0.0.1:{{port}}/data/wiki/get/{{wiki_id}}
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 404

# Authenticated non-member still cannot use the public write hole
POST http://127.0.0.1:{{port}}/data/wiki/add
Authorization: Bearer {{carol_token}}
X-Project-Id: alice/testapp
X-Site-Id: open
Content-Type: application/json
{}
HTTP 403

# ...but can read a collection a public set grants read on
GET http://127.0.0.1:{{port}}/data/wiki/list
Authorization: Bearer {{carol_token}}
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true
