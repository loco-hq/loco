# Org ownership is the org membership row (issue #17).
#
# A project literally named `_` is a legal project name. Developer on that
# project is a project role: it does not grant creating further projects
# under the org, and it does not grant managing org members.

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

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "acme"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "acme"
jsonpath "$.data.type" == "org"

# Bob does not own acme.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "other", "label": "Other", "description": "Not Bob's org", "account": "acme"}
HTTP 403
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "you do not have access to this resource"

# Alice owns acme, so a project named `_` is created, not refused as access.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "_", "label": "Underscore", "description": "A real project", "account": "acme"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.project" == "acme/_"

# The owner can still create other projects once `_` exists.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"name": "crm", "label": "CRM", "description": "Acme CRM", "account": "acme"}
HTTP 201
[Asserts]
jsonpath "$.data.project" == "acme/crm"

# Bob becomes a developer of the project named `_`, and of nothing else.
POST http://127.0.0.1:{{port}}/config/member/acme/_
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "developer"}
HTTP 201
[Asserts]
jsonpath "$.data.role" == "developer"
jsonpath "$.data.project" == "acme/_"

# That project role still opens the project itself.
POST http://127.0.0.1:{{port}}/schema/acme/_/0.0.1-dev/collection
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "lots", "label": "Lot", "label_plural": "Lots"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

# It does not open creating another project under the org.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "sneak", "label": "Sneak", "description": "Should stay forbidden", "account": "acme"}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

# Org member routes require the owner role. Bob is a project developer only.
GET http://127.0.0.1:{{port}}/config/org/acme/member/list
Authorization: Bearer {{bob_token}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"handle": "bob", "role": "owner"}
HTTP 403

DELETE http://127.0.0.1:{{port}}/config/org/acme/member/alice
Authorization: Bearer {{bob_token}}
HTTP 403

# Alice, the owner, lists, invites, and removes.
GET http://127.0.0.1:{{port}}/config/org/acme/member/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[?(@.handle == 'alice')].role" nth 0 == "owner"

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "member"}
HTTP 201
[Asserts]
jsonpath "$.data.handle" == "bob"
jsonpath "$.data.role" == "member"
jsonpath "$.data.org" == "acme"

GET http://127.0.0.1:{{port}}/config/org/acme/member/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[?(@.handle == 'bob')].role" nth 0 == "member"

# An org member who is not an owner still cannot create projects or
# manage members, even while they develop `acme/_`.
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"name": "sneak", "label": "Sneak", "description": "Still forbidden", "account": "acme"}
HTTP 403

GET http://127.0.0.1:{{port}}/config/org/acme/member/list
Authorization: Bearer {{bob_token}}
HTTP 403

POST http://127.0.0.1:{{port}}/config/org/acme/member
Authorization: Bearer {{bob_token}}
Content-Type: application/json
{"handle": "alice", "role": "member"}
HTTP 403

DELETE http://127.0.0.1:{{port}}/config/org/acme/member/alice
Authorization: Bearer {{bob_token}}
HTTP 403

DELETE http://127.0.0.1:{{port}}/config/org/acme/member/bob
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data" == "removed"

GET http://127.0.0.1:{{port}}/config/org/acme/member/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[?(@.handle == 'bob')]" count == 0

# A person account is not an org, so its owner does not pass this gate.
GET http://127.0.0.1:{{port}}/config/org/alice/member/list
Authorization: Bearer {{alice_token}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/config/org/alice/member
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{"handle": "bob", "role": "member"}
HTTP 403

# An org the caller does not own, including one that does not exist.
GET http://127.0.0.1:{{port}}/config/org/nope/member/list
Authorization: Bearer {{alice_token}}
HTTP 403
