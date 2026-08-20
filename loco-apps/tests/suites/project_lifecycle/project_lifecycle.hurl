# Project lifecycle integration test
# Verifies: creating a project auto-bootstraps a default "dev" site + dataset,
#           and deleting a project cascades to all its sites and datasets.
#
# Fixture: loco/cards hosts the metadata-editor "cards" site that alice logs
# into; alice/bootstrap is a pre-existing project owned by alice that should
# survive the cascade. The project under test is alice/newapp, which does
# not exist at startup.

# Login as alice (global). Schema/config writes still send loco/cards headers.
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Create a new project at alice/newapp
POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{
  "name": "newapp",
  "label": "New App",
  "description": "Freshly created project"
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App"
jsonpath "$.data.description" == "Freshly created project"

# 2. GET the project back — verify it persists with the right fields
GET http://127.0.0.1:{{port}}/config/project/alice/newapp
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App"
jsonpath "$.data.description" == "Freshly created project"

# 3. The default "dev" dataset should have been auto-created
GET http://127.0.0.1:{{port}}/config/dataset/alice/newapp/dev
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App Dev"
jsonpath "$.data.description" == "Default development dataset"

# 4. The default "dev" site should have been auto-created
GET http://127.0.0.1:{{port}}/config/site/alice/newapp/dev
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App Dev"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

# 4b. The default "0.0.1-dev" version (manifest) should have been auto-created
GET http://127.0.0.1:{{port}}/schema/alice/newapp/0.0.1-dev/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dependencies" count == 0

# 5. Add an extra dataset and site so we can verify the cascade covers more than
#    just the bootstrap pair.
POST http://127.0.0.1:{{port}}/config/dataset/alice/newapp
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{
  "name": "prod",
  "label": "New App Prod",
  "description": "Production dataset"
}
HTTP 201

POST http://127.0.0.1:{{port}}/config/site/alice/newapp
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
Content-Type: application/json
{
  "name": "prod",
  "label": "New App Prod",
  "version": "1.0.0",
  "dataset": "prod"
}
HTTP 201

# 6. Sanity: the prod pair is reachable individually before we delete
GET http://127.0.0.1:{{port}}/config/dataset/alice/newapp/prod
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.label" == "New App Prod"

GET http://127.0.0.1:{{port}}/config/site/alice/newapp/prod
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.data.version" == "1.0.0"

# 7. Delete the project — should cascade to all sites and datasets under it
DELETE http://127.0.0.1:{{port}}/config/project/alice/newapp
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 8. Project is gone (404 is now the project-existence check inside the scope)
GET http://127.0.0.1:{{port}}/config/project/alice/newapp
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

# 9. Both datasets are gone
GET http://127.0.0.1:{{port}}/config/dataset/alice/newapp/dev
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

GET http://127.0.0.1:{{port}}/config/dataset/alice/newapp/prod
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

# 9b. The auto-bootstrapped manifest is gone too (project delete cascades versions)
GET http://127.0.0.1:{{port}}/schema/alice/newapp/0.0.1-dev/manifest
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

# 10. Both sites are gone
GET http://127.0.0.1:{{port}}/config/site/alice/newapp/dev
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

GET http://127.0.0.1:{{port}}/config/site/alice/newapp/prod
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 404

# 11. The pre-existing project alice/bootstrap (and its cards site) is untouched
GET http://127.0.0.1:{{port}}/config/project/alice/bootstrap
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200

GET http://127.0.0.1:{{port}}/config/site/alice/bootstrap/cards
Authorization: Bearer {{token}}
X-Project-Id: loco/cards
X-Site-Id: cards
HTTP 200
