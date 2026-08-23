# Schema GET vs write. Reads: developer/editor any version; public on a
# site that assigns at least one permission set, pinned version only.
# Writes stay developer + draft.
#
# Fixtures (shared with data_no_auth.hurl):
#   site `dev`  — version 0-draft, public_permission_sets: [guestbook_read, guestbook_create]
#   site `open` — version 0-draft, public_permission_sets: [secrets_read, wiki_public]
#   site `cards` — version 0-draft, no public sets
#   widgets has a `size` field even though it has no public data grant.

# 1. Token-less GET of the pinned version on a public site.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/guestbook/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "message"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 4

# Any assigned set → the whole pinned version (widgets has no public data grant).
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/widgets/list
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "size"

# 2. Site with no public sets is not a public schema read.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/guestbook/list
X-Project-Id: alice/testapp
X-Site-Id: cards
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 3. Path version must match the site pin.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/1.0.0/field/guestbook/list
X-Project-Id: alice/testapp
X-Site-Id: open
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 4. Header project must match the path project.
GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
X-Project-Id: bob/myapp
X-Site-Id: cards
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 5. Writes stay gated — token-less POST is 401 even with public site headers.
POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{
  "name": "sneaky",
  "label": "Sneaky",
  "label_plural": "Sneakies"
}
HTTP 401
[Asserts]
jsonpath "$.ok" == false

PUT http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/guestbook/message
X-Project-Id: alice/testapp
X-Site-Id: dev
Content-Type: application/json
{"label": "Nope"}
HTTP 401
[Asserts]
jsonpath "$.ok" == false

# 6. Authenticated non-member: 403 without headers, public read with headers.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "carol", "password": "password"}
HTTP 200
[Captures]
carol_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/guestbook/list
Authorization: Bearer {{carol_token}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/guestbook/list
Authorization: Bearer {{carol_token}}
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data[0].name" == "message"

POST http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection
Authorization: Bearer {{carol_token}}
Content-Type: application/json
{
  "name": "sneaky",
  "label": "Sneaky",
  "label_plural": "Sneakies"
}
HTTP 403
[Asserts]
jsonpath "$.ok" == false

# 7. Developer can still GET any version without site headers.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
alice_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/field/guestbook/list
Authorization: Bearer {{alice_token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data[0].name" == "message"
