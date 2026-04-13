# Project lifecycle integration test
# Verifies: creating a project auto-bootstraps a default "dev" site + dataset,
#           and deleting a project cascades to all its sites and datasets.
#
# Fixture: alice owns alice/bootstrap (just hosts the "cards" config site for login).
# The project under test is alice/newapp, which does not exist at startup.

# Login as alice
POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "site_id": "cards"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

# 1. Create a new project at alice/newapp
POST http://127.0.0.1:{{port}}/config/create/project/alice/newapp/project
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "fields": {
    "label": "New App",
    "description": "Freshly created project"
  }
}
HTTP 201
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App"
jsonpath "$.data.description" == "Freshly created project"

# 2. GET the project back — verify it persists with the right fields
GET http://127.0.0.1:{{port}}/config/get/project/alice/newapp/project
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App"
jsonpath "$.data.description" == "Freshly created project"

# 3. The default "dev" dataset should have been auto-created
GET http://127.0.0.1:{{port}}/config/get/dataset/alice/newapp/datasets/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App Dev"
jsonpath "$.data.description" == "Default development dataset"

# 4. The default "dev" site should have been auto-created
GET http://127.0.0.1:{{port}}/config/get/site/alice/newapp/sites/dev
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true
jsonpath "$.data.label" == "New App Dev"
jsonpath "$.data.version" == "0.0.1-dev"
jsonpath "$.data.dataset" == "dev"

# 5. Add an extra dataset and site so we can verify the cascade covers more than
#    just the bootstrap pair.
POST http://127.0.0.1:{{port}}/config/create/dataset/alice/newapp/datasets/prod
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "fields": {
    "label": "New App Prod",
    "description": "Production dataset"
  }
}
HTTP 201

POST http://127.0.0.1:{{port}}/config/create/site/alice/newapp/sites/prod
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "fields": {
    "label": "New App Prod",
    "version": "1.0.0",
    "dataset": "prod"
  }
}
HTTP 201

# 6. Sanity: the prod pair is reachable individually before we delete
GET http://127.0.0.1:{{port}}/config/get/dataset/alice/newapp/datasets/prod
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.label" == "New App Prod"

GET http://127.0.0.1:{{port}}/config/get/site/alice/newapp/sites/prod
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.version" == "1.0.0"

# 7. Delete the project — should cascade to all sites and datasets under it
DELETE http://127.0.0.1:{{port}}/config/delete/project/alice/newapp/project
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.ok" == true

# 8. Project is gone
GET http://127.0.0.1:{{port}}/config/get/project/alice/newapp/project
Authorization: Bearer {{token}}
HTTP 404

# 9. Both datasets are gone
GET http://127.0.0.1:{{port}}/config/get/dataset/alice/newapp/datasets/dev
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/config/get/dataset/alice/newapp/datasets/prod
Authorization: Bearer {{token}}
HTTP 404

# 10. Both sites are gone
GET http://127.0.0.1:{{port}}/config/get/site/alice/newapp/sites/dev
Authorization: Bearer {{token}}
HTTP 404

GET http://127.0.0.1:{{port}}/config/get/site/alice/newapp/sites/prod
Authorization: Bearer {{token}}
HTTP 404

# 11. The bootstrap project alice/bootstrap (and its cards site) is untouched
GET http://127.0.0.1:{{port}}/config/get/project/alice/bootstrap/project
Authorization: Bearer {{token}}
HTTP 200

GET http://127.0.0.1:{{port}}/config/get/site/alice/bootstrap/sites/cards
Authorization: Bearer {{token}}
HTTP 200
