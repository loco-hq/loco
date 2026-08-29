# Hosting: a site URL serves its pinned version's bundle.
#
# Fixture: alice owns alice/blog (person-owned → implicit developer) with
# three sites — `www` pins draft 0-dev (empty until this suite deploys to it),
# `live` pins published 1.0.0 (a tree already on disk), and `bare` pins 2-dev,
# which never gets a bundle at all.
#
# Every request below aims at 127.0.0.1 and carries the derived subdomain in
# `Host`, which is exactly what a browser resolving
# `www.blog.alice.localhost` to this process does. No LOCO_DEFAULT_SITE here:
# the apex stays API-only.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. The apex has no site and no default, so there is nothing at `/`. JSON,
#    because a 404 from Loco is always something a client can parse.
GET http://127.0.0.1:{{port}}/
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false
jsonpath "$.error" contains "no such endpoint"

# 2. Neither does a site whose version was never built.
GET http://127.0.0.1:{{port}}/
Host: www.blog.alice.localhost:{{port}}
HTTP 404
[Asserts]
jsonpath "$.ok" == false

# 3. Deploy: a zip of a Vite dist onto the draft the site pins.
PUT http://127.0.0.1:{{port}}/schema/alice/blog/0-dev/bundle
Authorization: Bearer {{token}}
Content-Type: application/zip
file,dist.zip;
HTTP 200
[Asserts]
jsonpath "$.data.files" == 3

# 4. That URL is now the blog. No token: a snapshot's face is public.
GET http://127.0.0.1:{{port}}/
Host: www.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" == "text/html; charset=utf-8"
header "Cache-Control" == "no-cache"
body contains "draft blog"

# 5. Hashed assets come out of the same tree with their own content types.
GET http://127.0.0.1:{{port}}/assets/app-9f8e7d.js
Host: www.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" == "text/javascript; charset=utf-8"
body contains "console.log(\"draft\")"

GET http://127.0.0.1:{{port}}/assets/logo.svg
Host: www.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" == "image/svg+xml"

# 6. SPA fallback: a client-side route is extensionless, so it gets the shell.
GET http://127.0.0.1:{{port}}/posts/hello-world
Host: www.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
header "Content-Type" == "text/html; charset=utf-8"
body contains "draft blog"

# A browser navigation asks for HTML explicitly; that is enough on its own.
GET http://127.0.0.1:{{port}}/archive.html
Host: www.blog.alice.localhost:{{port}}
Accept: text/html,application/xhtml+xml
HTTP 200
[Asserts]
body contains "draft blog"

# 7. A missing hashed asset is a 404. Answering it with the HTML shell would
#    turn a bad deploy into a syntax error in the browser console.
GET http://127.0.0.1:{{port}}/assets/missing.js
Host: www.blog.alice.localhost:{{port}}
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.ok" == false

# 8. Reserved prefixes win on a site host too — JSON, never the shell.
GET http://127.0.0.1:{{port}}/data/does-not-exist/list
Host: www.blog.alice.localhost:{{port}}
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.error" contains "unknown collection"

GET http://127.0.0.1:{{port}}/schema/not/a/real/route
Host: www.blog.alice.localhost:{{port}}
Accept: text/html
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"

# 9. The host filled in the site headers, which is why step 8 got as far as
#    "unknown collection" instead of "missing project". Sending them is still
#    fine when they agree.
GET http://127.0.0.1:{{port}}/data/does-not-exist/list
Host: www.blog.alice.localhost:{{port}}
X-Project-Id: alice/blog
X-Site-Id: www
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown collection"

# Disagreeing with the URL is not fine: one site's address may not reach
# another site's data.
GET http://127.0.0.1:{{port}}/data/does-not-exist/list
Host: www.blog.alice.localhost:{{port}}
X-Project-Id: alice/blog
X-Site-Id: live
HTTP 400
[Asserts]
jsonpath "$.error" contains "this host serves alice/blog/www"

# 10. A published version's bytes never change, so its assets cache forever —
#     while `index.html` stays a pointer and revalidates, so a repin shows up.
GET http://127.0.0.1:{{port}}/
Host: live.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
header "Cache-Control" == "no-cache"
body contains "published blog"

GET http://127.0.0.1:{{port}}/assets/app-1a2b3c.js
Host: live.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
header "Cache-Control" == "public, max-age=31536000, immutable"
body contains "published"

# 11. A site whose version has no bundle 404s. The process booted anyway —
#     every request above it proves that.
GET http://127.0.0.1:{{port}}/
Host: bare.blog.alice.localhost:{{port}}
HTTP 404

# 12. A host that names no site is not a site host, so it falls back to the
#     apex rule — and with no default site, that is API-only.
GET http://127.0.0.1:{{port}}/
Host: nosuch.blog.alice.localhost:{{port}}
HTTP 404

GET http://127.0.0.1:{{port}}/
Host: blog.alice.localhost:{{port}}
HTTP 404

# 13. Only GET and HEAD read the tree; a write to a static path is not an
#     endpoint.
POST http://127.0.0.1:{{port}}/
Host: www.blog.alice.localhost:{{port}}
HTTP 404
[Asserts]
jsonpath "$.error" contains "no such endpoint"

# 14. Nothing escapes the tree, encoded or not.
GET http://127.0.0.1:{{port}}/assets/%2e%2e%2f%2e%2e%2fmanifest.yaml
Host: www.blog.alice.localhost:{{port}}
HTTP 404

# 15. Undeploying takes the site down without touching the site or the data.
DELETE http://127.0.0.1:{{port}}/schema/alice/blog/0-dev/bundle
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/
Host: www.blog.alice.localhost:{{port}}
HTTP 404
