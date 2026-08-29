# The apex as a host: LOCO_DEFAULT_SITE names the one site this process is.
#
# `cargo run` plus a default site is the "one process is the blog" case — and
# the generic form of what issue #30 originally wanted for Studio. The binary
# has no opinion about which site that is; this suite pins
# `alice/blog/www` through AppOptions the way the environment would.
#
# Fixture: alice/blog with two published versions that already carry bundles,
# pinned by sites `www` (the default) and `next`.

# 1. The apex serves the default site's pinned version at `/`, with no token
#    and no headers.
GET http://127.0.0.1:{{port}}/
HTTP 200
[Asserts]
header "Content-Type" == "text/html; charset=utf-8"
header "Cache-Control" == "no-cache"
body contains "the blog"

# 2. Its assets come from the same version, cacheable forever because 1.0.0's
#    bytes can never change.
GET http://127.0.0.1:{{port}}/assets/app-1a2b3c.js
HTTP 200
[Asserts]
header "Cache-Control" == "public, max-age=31536000, immutable"
body contains "console.log(\"blog\")"

# 3. SPA fallback works here exactly as it does on a subdomain.
GET http://127.0.0.1:{{port}}/posts/hello
HTTP 200
[Asserts]
body contains "the blog"

GET http://127.0.0.1:{{port}}/assets/missing.js
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"

# 4. The default also fills in absent site headers, so the hosted frontend can
#    call /data without knowing its own address.
GET http://127.0.0.1:{{port}}/data/does-not-exist/list
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown collection"

# 5. But it is a default, not a constraint: the apex is also where Studio and
#    local Vite apps talk to the API about some *other* site, so headers that
#    are actually sent win.
GET http://127.0.0.1:{{port}}/data/does-not-exist/list
X-Project-Id: alice/nosuchproject
X-Site-Id: www
HTTP 404
[Asserts]
jsonpath "$.error" contains "unknown site"

# 6. A host that names a site of its own beats the apex default.
GET http://127.0.0.1:{{port}}/
Host: next.blog.alice.localhost:{{port}}
HTTP 200
[Asserts]
body contains "the next blog"
