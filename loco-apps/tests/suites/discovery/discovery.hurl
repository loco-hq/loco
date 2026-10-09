# Discovery document, prefix catalogs, and a publish an agent can copy.
# dist.zip is index.html at the zip root (the file next to this suite).
# The page logs in. Writes need its token. Public read is a later opt-in.

GET http://127.0.0.1:{{port}}/.well-known/loco.json
HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
header "Cache-Control" == "no-cache"
jsonpath "$.name" == "loco"
jsonpath "$.version" isString
jsonpath "$.docs" count == 0
jsonpath "$.discovery" == "/.well-known/loco.json"
jsonpath "$.signup.handle" contains "1-63"
jsonpath "$.signup.handle" contains "a-z"
jsonpath "$.signup.rejected_handle" contains "400"
jsonpath "$.signup.rejected_handle" contains "1-63"
jsonpath "$.signup.missing_field" contains "400"
jsonpath "$.signup.missing_field" contains "password is required"
jsonpath "$.routes" count == 51
jsonpath "$.prefixes" count == 5
jsonpath "$.prefixes[0].routes" not exists
jsonpath "$.site.example" == "http://dev.shelf.book_club.localhost:{{port}}/"
jsonpath "$.site.url" == "http://{site}.{project}.{account}.localhost:{{port}}/"
jsonpath "$.site.note" contains "wildcard DNS"
jsonpath "$.site.note" contains "LOCO_DEFAULT_SITE"
jsonpath "$.site.note" contains "#145"
jsonpath "$.guide[8].do" contains "http://dev.shelf.book_club.localhost:{{port}}/"
jsonpath "$.guide[9].do" contains "http://www.shelf.book_club.localhost:{{port}}/"
jsonpath "$.guide[8].do" not contains "{port}"
jsonpath "$.site.example" not contains ":3000"
jsonpath "$.guide[5].do" contains "editor"
jsonpath "$.guide[5].do" contains "anyone with the URL"
jsonpath "$.guide[7].index_html" contains "/auth/login"
jsonpath "$.guide[7].index_html" contains "Authorization"
jsonpath "$.guide[7].index_html" contains "Bearer"
jsonpath "$.guide[7].index_html" contains "/auth/logout"
jsonpath "$.routes[0].method" == "GET"
jsonpath "$.routes[0].path" == "/.well-known/loco.json"
body not contains "lego_reseller"
body not contains "A hosted page sends no token"
body not contains "parts"

GET http://127.0.0.1:{{port}}/llms.txt
HTTP 200
[Asserts]
header "Content-Type" contains "text/plain"
body contains "/.well-known/loco.json"
body not contains "no such endpoint"

# A bare prefix names its routes. The error is not "no such endpoint".
GET http://127.0.0.1:{{port}}/auth
HTTP 404
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.see" == "/.well-known/loco.json"
body contains "POST /auth/users"
body contains "POST /auth/login"
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "/schema/"
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "/data/"
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "/config/"
body not contains "no such endpoint"

# An unknown path under a prefix keeps the old error and gains see.
GET http://127.0.0.1:{{port}}/schema/foo
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"
jsonpath "$.see" == "/.well-known/loco.json"

GET http://127.0.0.1:{{port}}/help
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"
jsonpath "$.see" == "/.well-known/loco.json"

# GET /actions is the list, so a missing site is 400, not the prefix catalog.
GET http://127.0.0.1:{{port}}/actions
HTTP 400
[Asserts]
jsonpath "$.error" contains "missing project"

# Signup, as main answers it. A hyphen in the handle is 400 naming the
# account-name rule. A missing field is 400 JSON naming the field. An empty
# password is 400. A taken handle is 409. A wrong password at login is 401.
POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "book-club", "name": "Book Club", "password": "pass-word-1"}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" == "handle name \"book-club\" must be 1-63 characters of a-z, 0-9, and _, starting with a letter or _"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "username"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "book_club", "name": "Book Club"}
HTTP 400
[Asserts]
jsonpath "$.error" == "password is required"

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "book_club", "name": "Book Club", "password": "pass-word-1"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.username" == "book_club"
jsonpath "$.data.id" isString
jsonpath "$.data.token" not exists

POST http://127.0.0.1:{{port}}/auth/users
Content-Type: application/json
{"username": "book_club", "name": "Book Club", "password": "pass-word-1"}
HTTP 409
[Asserts]
jsonpath "$.error" == "user already exists"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "book_club", "password": "wrong-password"}
HTTP 401
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "invalid credentials"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{}
HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "username"

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "book_club", "password": "pass-word-1"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"
[Asserts]
jsonpath "$.data.user.username" == "book_club"

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "shelf", "label": "Shelf", "description": "A reading list"}
HTTP 201
[Asserts]
jsonpath "$.ok" == true

GET http://127.0.0.1:{{port}}/config/site/book_club/shelf/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

POST http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "books", "label": "Book", "label_plural": "Books"}
HTTP 201
[Asserts]
jsonpath "$.data.name" == "books"

POST http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "books", "name": "title", "type": "string", "label": "Title", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "books", "name": "author", "type": "string", "label": "Author", "required": true}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/field
Authorization: Bearer {{token}}
Content-Type: application/json
{"collection": "books", "name": "finished", "type": "boolean", "label": "Finished", "required": false}
HTTP 201

# Owner writes need no public grant.
POST http://127.0.0.1:{{port}}/data/books/add
Authorization: Bearer {{token}}
X-Project-Id: book_club/shelf
X-Site-Id: dev
Content-Type: application/json
{"title": "Dune", "author": "Herbert"}
HTTP 201
[Captures]
record: jsonpath "$.data.id"
[Asserts]
jsonpath "$.data.fields.title" == "Dune"
jsonpath "$.data.fields.author" == "Herbert"

PUT http://127.0.0.1:{{port}}/data/books/update/{{record}}
Authorization: Bearer {{token}}
X-Project-Id: book_club/shelf
X-Site-Id: dev
Content-Type: application/json
{"finished": true}
HTTP 200
[Asserts]
jsonpath "$.data.fields.finished" == true
jsonpath "$.data.fields.title" == "Dune"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: book_club/shelf
X-Site-Id: dev
Content-Type: application/json
{"queries": {"books": {"collection": "books", "limit": 50}}}
HTTP 200
[Asserts]
jsonpath "$.data.books.records" count == 1
jsonpath "$.data.books.records[0].fields.title" == "Dune"
jsonpath "$.data.books.cursor" == null

PUT http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
[Asserts]
jsonpath "$.data.files" == 1
jsonpath "$.data.hash" isString

GET http://127.0.0.1:{{port}}/
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" contains "text/html"
body contains "discovery page"
body contains "/auth/login"

GET http://127.0.0.1:{{port}}/.well-known/loco.json
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.name" == "loco"
jsonpath "$.site.example" == "http://dev.shelf.book_club.localhost:{{port}}/"

GET http://127.0.0.1:{{port}}/llms.txt
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" contains "text/plain"
body contains "/.well-known/loco.json"

GET http://127.0.0.1:{{port}}/auth
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 404
[Asserts]
jsonpath "$.see" == "/.well-known/loco.json"
body contains "POST /auth/users"
body not contains "no such endpoint"
body not contains "discovery page"

GET http://127.0.0.1:{{port}}/schema/foo
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"
jsonpath "$.see" == "/.well-known/loco.json"

# The page's calls, on the site host, with no site headers.
# Without the token they are 403. With it they succeed.
GET http://127.0.0.1:{{port}}/data/books/list
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 403
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/data/books/add
Host: dev.shelf.book_club.localhost:{{port}}
Content-Type: application/json
{"title": "Emma", "author": "Austen"}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

PUT http://127.0.0.1:{{port}}/data/books/update/{{record}}
Host: dev.shelf.book_club.localhost:{{port}}
Content-Type: application/json
{"finished": true}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/data/books/add
Host: dev.shelf.book_club.localhost:{{port}}
Authorization: Bearer {{token}}
Content-Type: application/json
{"title": "Emma", "author": "Austen"}
HTTP 201
[Captures]
emma: jsonpath "$.data.id"
[Asserts]
jsonpath "$.data.fields.title" == "Emma"

PUT http://127.0.0.1:{{port}}/data/books/update/{{emma}}
Host: dev.shelf.book_club.localhost:{{port}}
Authorization: Bearer {{token}}
Content-Type: application/json
{"finished": true}
HTTP 200
[Asserts]
jsonpath "$.data.fields.finished" == true
jsonpath "$.data.fields.title" == "Emma"

POST http://127.0.0.1:{{port}}/config/version/book_club/shelf
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "0.0.1", "from": "0.0.1-dev"}
HTTP 201
[Asserts]
jsonpath "$.data.version" == "0.0.1"

POST http://127.0.0.1:{{port}}/config/site/book_club/shelf
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "www", "label": "Shelf", "version": "0.0.1", "dataset": "dev"}
HTTP 201

GET http://127.0.0.1:{{port}}/
Host: www.shelf.book_club.localhost:{{port}}
HTTP 200
[Asserts]
body contains "discovery page"

# Published site: still members only. The copy assigned no public set.
GET http://127.0.0.1:{{port}}/data/books/list
Host: www.shelf.book_club.localhost:{{port}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/data/books/add
Host: www.shelf.book_club.localhost:{{port}}
Content-Type: application/json
{"title": "Persuasion", "author": "Austen"}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

POST http://127.0.0.1:{{port}}/data/books/add
Host: www.shelf.book_club.localhost:{{port}}
Authorization: Bearer {{token}}
Content-Type: application/json
{"title": "Persuasion", "author": "Austen"}
HTTP 201
[Asserts]
jsonpath "$.data.fields.title" == "Persuasion"

# Opt in to public read on the draft. Create and update stay false.
POST http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "books_read",
  "label": "Readers",
  "collections": [
    {"collection": "books", "read": true, "create": false, "update": false, "delete": false}
  ]
}
HTTP 201
[Asserts]
jsonpath "$.data.collections[0].read" == true
jsonpath "$.data.collections[0].create" == false
jsonpath "$.data.collections[0].update" == false
jsonpath "$.data.collections[0].delete" == false

PUT http://127.0.0.1:{{port}}/schema/book_club/shelf/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"public_permission_sets": ["books_read"]}
HTTP 200
[Asserts]
jsonpath "$.data.public_permission_sets[0]" == "books_read"

GET http://127.0.0.1:{{port}}/data/books/list
Host: dev.shelf.book_club.localhost:{{port}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
body contains "Dune"

POST http://127.0.0.1:{{port}}/data/books/add
Host: dev.shelf.book_club.localhost:{{port}}
Content-Type: application/json
{"title": "Emma", "author": "Austen"}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

# The published version was copied before the opt-in, so it stays members only.
GET http://127.0.0.1:{{port}}/data/books/list
Host: www.shelf.book_club.localhost:{{port}}
HTTP 403
[Asserts]
jsonpath "$.error" == "you do not have access to this resource"

# Mount check. discovery.rs asserts each of these request lines exists.
# A body that says "no such endpoint" means the path is not mounted.
# status != 405 means this method is mounted: a wrong method is 405 with an empty body.
# The discovery document itself contains "no such endpoint", so its probe is 200.
POST http://127.0.0.1:{{port}}/auth/login
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/auth/logout
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/auth/me
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/auth/users
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/auth/users/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/auth/users/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/auth/api-keys
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/auth/api-keys/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/auth/api-keys/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/project
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/project/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/project/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/dataset/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/dataset/x/x/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/dataset/x/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/site/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/site/x/x/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/site/x/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/config/site/x/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/config/version/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/config/version/x/x/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/manifest
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/manifest
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/schema/x/x/x/collection
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/collection/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/collection/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/collection/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/schema/x/x/x/collection/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/schema/x/x/x/field
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/field/x/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/field/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/schema/x/x/x/field/x/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/schema/x/x/x/permission_set
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/permission_set/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/permission_set/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/permission_set/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/schema/x/x/x/bundle
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/schema/x/x/x/bundle
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/schema/x/x/x/bundle
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/data/x/add
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data/x/list
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data/x/get/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

PUT http://127.0.0.1:{{port}}/data/x/update/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

DELETE http://127.0.0.1:{{port}}/data/x/delete/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/data/x/fields
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/data/query
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/actions
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/actions/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

POST http://127.0.0.1:{{port}}/actions/x
HTTP *
[Asserts]
status != 405
body not contains "no such endpoint"

GET http://127.0.0.1:{{port}}/.well-known/loco.json
HTTP 200
[Asserts]
status != 405

GET http://127.0.0.1:{{port}}/llms.txt
HTTP 200
[Asserts]
status != 405
body not contains "no such endpoint"
