# Sessions are bounded. A login is good for LocalAuthAdapter::SESSION_TTL_DAYS
# and is written to disk with an absolute `expires_at`; logging out drops it.
# `suite_auth_sessions` in tests/hurl_runner.rs asserts on the session files
# this suite leaves behind — keep the login/logout count in step with it.
#
# POST /auth/login  — issues a bounded session
# POST /auth/logout — ends it immediately
# GET  /auth/me     — the ended session no longer authenticates

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
first_token: jsonpath "$.data.token"

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{first_token}}
HTTP 200
[Asserts]
jsonpath "$.data.username" == "alice"

POST http://127.0.0.1:{{port}}/auth/logout
Authorization: Bearer {{first_token}}
HTTP 200

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{first_token}}
HTTP 401
[Asserts]
jsonpath "$.error" == "invalid or expired session"

# A fresh login still works after all that.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
second_token: jsonpath "$.data.token"
[Asserts]
jsonpath "$.data.token" != "{{first_token}}"

GET http://127.0.0.1:{{port}}/auth/me
Authorization: Bearer {{second_token}}
HTTP 200
