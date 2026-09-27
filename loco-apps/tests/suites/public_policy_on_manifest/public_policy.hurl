# Public policy is a property of the version, not of the URL that pins it.
#
# `public_permission_sets` lives on the version's manifest. A site is a URL
# pointing at (version, dataset), so two sites pinning one version cannot
# disagree about what `public` may do, and opting into a package's set is a
# choice each *consuming version* makes for itself.
#
# Fixtures:
#   acme/crm@1.0        — collection `contacts`, and the recommended set
#                         `public_contacts` (read + create on bare `contacts`,
#                         which from where it is written means acme/crm's).
#                         The package assigns nothing to public itself.
#   alice/agency@1.0-dev — depends on acme/crm@1.0, manifest assigns
#                         [acme/crm.public_contacts]
#   alice/agency@2.0-dev — depends on acme/crm@1.0, manifest assigns nothing
#   sites: `open` → 1.0-dev + dataset dev
#          `staging` → 1.0-dev + dataset staging  (same snapshot, other data)
#          `closed` → 2.0-dev + dataset dev
#
# Both consuming versions also declare their own `contacts` collection. The
# package's set opens the package's collection, `acme/crm.contacts` —
# addressed on /data as `acme%2Fcrm.contacts` — and never the consumer's
# same-named one.

# --- 1. The dependency really is installed, and its set is what public
#        assignment is naming.

GET http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/permission_set/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data[*].name" includes "public_contacts"
jsonpath "$.data[0].project" == "acme/crm"

# The consuming version declares no permission set of its own — the only one
# in scope is the package's.
GET http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/permission_set/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.data" count == 1

# --- 2. The version that lists the package's set grants `public` on the
#        package's collection.

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0

POST http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/add
X-Project-Id: alice/agency
X-Site-Id: open
Content-Type: application/json
{"email": "visitor@example.com"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.owner" == "public"
[Captures]
contact_id: jsonpath "$.data.id"

# The set grants read + create and nothing else. Unspecified verbs are false.
PUT http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/update/{{contact_id}}
X-Project-Id: alice/agency
X-Site-Id: open
Content-Type: application/json
{"email": "edited@example.com"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

DELETE http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/delete/{{contact_id}}
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 403

# The package's bare grant does not reach the consumer's own `contacts`.
GET http://127.0.0.1:{{port}}/data/contacts/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 403

POST http://127.0.0.1:{{port}}/data/contacts/add
X-Project-Id: alice/agency
X-Site-Id: open
Content-Type: application/json
{"email": "visitor@example.com"}
HTTP 403

# An assigned set also opens the pinned version to token-less /schema reads.
GET http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/collection/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# --- 3. The sibling version depends on the same package and lists nothing.
#        Same collection, same dataset, same package — no public access.

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: closed
HTTP 403
[Asserts]
jsonpath "$.ok" == false

POST http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/add
X-Project-Id: alice/agency
X-Site-Id: closed
Content-Type: application/json
{"email": "visitor@example.com"}
HTTP 403

GET http://127.0.0.1:{{port}}/schema/alice/agency/2.0-dev/collection/list
X-Project-Id: alice/agency
X-Site-Id: closed
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 2.0-dev is a real version with the same collection — the 403s above are
# the empty assignment, not a missing fixture.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/schema/alice/agency/2.0-dev/collection/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data[*].name" includes "contacts"

# --- 4. Two sites pinning the SAME version cannot disagree.
#        `staging` differs from `open` only by dataset.

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: staging
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 0

POST http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/add
X-Project-Id: alice/agency
X-Site-Id: staging
Content-Type: application/json
{"email": "staging@example.com"}
HTTP 201

# Same policy, different partition: `open` still sees only its own record.
GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].fields.email" == "visitor@example.com"

GET http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/collection/list
X-Project-Id: alice/agency
X-Site-Id: staging
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# --- 5. Policy is edited on the manifest (draft-only), and every site
#        pinning that version moves together.

GET http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/manifest
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data.public_permission_sets" count == 1
jsonpath "$.data.public_permission_sets[0]" == "acme/crm.public_contacts"

PUT http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/manifest
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"public_permission_sets": []}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.public_permission_sets" count == 0
jsonpath "$.data.dependencies[0]" == "acme/crm@1.0"

# One manifest write, both sites.
GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 403

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: staging
HTTP 403

# A bare name is the consuming version's own set. alice/agency has no
# `public_contacts`, so naming the package's set bare grants nothing.
PUT http://127.0.0.1:{{port}}/schema/alice/agency/2.0-dev/manifest
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"public_permission_sets": ["public_contacts"]}
HTTP 200

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: closed
HTTP 403

# And the sibling version can opt in the same way, qualified.
PUT http://127.0.0.1:{{port}}/schema/alice/agency/2.0-dev/manifest
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"public_permission_sets": ["acme/crm.public_contacts"]}
HTTP 200

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: closed
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# --- 6. There is no site-level override to fall back on. `site` no longer
#        declares the field, so a /config write naming it changes nothing.

PUT http://127.0.0.1:{{port}}/config/site/alice/agency/staging
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"label": "Staging", "public_permission_sets": ["acme/crm.public_contacts"]}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.public_permission_sets" not exists

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: staging
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# Unknown set names stay inert — nothing resolves them, so nothing is granted.
PUT http://127.0.0.1:{{port}}/schema/alice/agency/1.0-dev/manifest
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"public_permission_sets": ["no_such_set"]}
HTTP 200

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.contacts/list
X-Project-Id: alice/agency
X-Site-Id: open
HTTP 403
