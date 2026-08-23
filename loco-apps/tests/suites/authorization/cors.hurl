# CORS for a cross-origin page. Preflight must succeed without site headers
# (the browser sends those only on the actual request). Token-less GET of a
# public collection must echo Access-Control-Allow-Origin.
#
# Fixtures: site `dev` assigns guestbook_read (see data_no_auth.hurl).

# 1. Preflight — no X-Project-Id / X-Site-Id on this request.
OPTIONS http://127.0.0.1:{{port}}/data/guestbook/list
Origin: http://127.0.0.1:5176
Access-Control-Request-Method: GET
Access-Control-Request-Headers: x-project-id, x-site-id
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "*"
header "Access-Control-Allow-Headers" == "*"
header "Access-Control-Allow-Methods" == "*"

# 2. Actual list from another origin, no token.
GET http://127.0.0.1:{{port}}/data/guestbook/list
Origin: http://127.0.0.1:5176
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "*"
jsonpath "$.ok" == true

# 3. Public schema read also carries CORS (field labels, collection list).
OPTIONS http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Origin: http://127.0.0.1:5176
Access-Control-Request-Method: GET
Access-Control-Request-Headers: x-project-id, x-site-id
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "*"

GET http://127.0.0.1:{{port}}/schema/alice/testapp/0-draft/collection/list
Origin: http://127.0.0.1:5176
X-Project-Id: alice/testapp
X-Site-Id: dev
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "*"
jsonpath "$.ok" == true
