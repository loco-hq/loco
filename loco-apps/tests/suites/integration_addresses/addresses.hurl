# Resolve {project}.{integration}:{name} on /data, /actions, queries, and grants.
#
# acme/salesforce declares type salesforce (collection account, field status,
# type action set_owner) and integration prod. acme/crm declares ordinary
# collection foo.bar. ben/sync depends on both and declares sf_east and
# sf_west of that type. sf_east also declares custom invoice__c. alice/shop
# depends only on ben/sync, not on the type's project. A custom collection
# named account is refused: the type already offers it, and one address
# names one document.
#
# A resolved integration collection has no source yet. List, get, add,
# update, and delete are 501 after auth: the address resolved (not 404), the
# request is well formed (not 400), and a lake read would look like an empty
# collection. Fields are metadata and are returned. A query is no_source
# inside the 200, after the grant check, and does not call the lake. A
# collision is 409 after that same check; a caller without a grant is 403.
# An action address is never ambiguous.

POST http://127.0.0.1:{{port}}/auth/login
Content-Type: application/json
{"username": "alice", "password": "password"}
HTTP 200
[Captures]
token: jsonpath "$.data.token"

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{token}}
Content-Type: application/json
{"handle": "acme"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "salesforce", "label": "Salesforce", "description": "Type", "account": "acme"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "crm", "label": "CRM", "description": "Dotted name", "account": "acme"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/org
Authorization: Bearer {{token}}
Content-Type: application/json
{"handle": "ben"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "sync", "label": "Sync", "description": "Installer", "account": "ben"}
HTTP 201

POST http://127.0.0.1:{{port}}/config/project
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "shop", "label": "Shop", "description": "Consumer"}
HTTP 201

# --- Type, standard collection, type action, and the type's own integration.
# Collections and actions are lists on the type document. ---

POST http://127.0.0.1:{{port}}/schema/acme/salesforce/0.0.1-dev/integration_type
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "salesforce",
  "label": "Salesforce",
  "collections": [
    {
      "name": "account",
      "label": "Account",
      "label_plural": "Accounts",
      "fields": [{"name": "status", "type": "string", "label": "Status"}]
    }
  ],
  "actions": [
    {
      "name": "set_owner",
      "label": "Set owner",
      "description": "Sets the account owner",
      "params": [
        {"name": "owner_id", "type": "string", "label": "Owner", "required": true}
      ]
    }
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/acme/salesforce/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "prod", "label": "Prod", "type": "salesforce"}
HTTP 201

# A name with no colon is not the type's account.
POST http://127.0.0.1:{{port}}/schema/acme/crm/0.0.1-dev/collection
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "foo.bar", "label": "Foo bar", "label_plural": "Foo bars"}
HTTP 201

PUT http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["acme/salesforce@0.0.1-dev", "acme/crm@0.0.1-dev"], "public_permission_sets": []}
HTTP 200

POST http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "sf_east",
  "label": "East",
  "type": "acme/salesforce.salesforce",
  "collections": [
    {
      "name": "invoice__c",
      "label": "Invoice",
      "label_plural": "Invoices",
      "fields": [{"name": "amount", "type": "integer", "label": "Amount"}]
    }
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/integration
Authorization: Bearer {{token}}
Content-Type: application/json
{"name": "sf_west", "label": "West", "type": "acme/salesforce.salesforce"}
HTTP 201

# Same name as the standard collection. Naming collections replaces the list,
# so invoice__c is sent again. One address names one document, so the write
# is refused and the stored list stays invoice__c.
PUT http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/integration/sf_east
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "collections": [
    {"name": "invoice__c", "label": "Invoice", "label_plural": "Invoices"},
    {"name": "account", "label": "Custom account", "label_plural": "Custom accounts"}
  ]
}
HTTP 400
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.data" not exists
jsonpath "$.error" == "ambiguous address sf_east:account: type 'acme/salesforce.salesforce' offers collection 'account' and integration 'sf_east' declares it"

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/integration/sf_east
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.collections" count == 1
jsonpath "$.data.collections[0].name" == "invoice__c"

PUT http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["ben/sync@0.0.1-dev"], "public_permission_sets": []}
HTTP 200

POST http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "east",
  "label": "East accounts",
  "collections": [
    {"collection": "sf_east:account", "read": true}
  ]
}
HTTP 201

POST http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/permission_set
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "name": "bare_account",
  "label": "Bare account",
  "collections": [
    {"collection": "account", "read": true}
  ]
}
HTTP 201

# --- A name with no colon does not reach the type. The document routes stay ordinary. ---

GET http://127.0.0.1:{{port}}/data/account/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: account"

GET http://127.0.0.1:{{port}}/data/acme%2Fsalesforce.account/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: acme/salesforce.account"

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/collection/sf_east:account
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" == "collection not found: sf_east:account"

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/action/sf_east:set_owner
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" == "action not found: sf_east:set_owner"

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/action/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

GET http://127.0.0.1:{{port}}/data/sf_east:account:extra/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: sf_east:account:extra"

GET http://127.0.0.1:{{port}}/data/prod:account/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: prod:account"

# --- Data verbs are 501. Fields are not. sf_west has no invoice__c. ---

GET http://127.0.0.1:{{port}}/data/sf_east:account/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.ok" == false
jsonpath "$.error" == "no source for collection sf_east:account"
jsonpath "$.data" not exists

GET http://127.0.0.1:{{port}}/data/sf_east:account/get/1
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection sf_east:account"

POST http://127.0.0.1:{{port}}/data/sf_east:account/add
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection sf_east:account"

PUT http://127.0.0.1:{{port}}/data/sf_east:account/update/1
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{}
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection sf_east:account"

DELETE http://127.0.0.1:{{port}}/data/sf_east:account/delete/1
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection sf_east:account"

GET http://127.0.0.1:{{port}}/data/sf_east:account/fields
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "status"
jsonpath "$.data[0].type" == "string"

GET http://127.0.0.1:{{port}}/data/sf_east:invoice__c/fields
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 1
jsonpath "$.data[0].name" == "amount"
jsonpath "$.data[0].type" == "integer"

GET http://127.0.0.1:{{port}}/data/sf_west:invoice__c/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: sf_west:invoice__c"

# The type's own integration, qualified. Bare prod:account is not ben/sync's.
GET http://127.0.0.1:{{port}}/data/acme%2Fsalesforce.prod:account/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection acme/salesforce.prod:account"

# A caller that does not depend on the type still addresses the installer's integration.
GET http://127.0.0.1:{{port}}/data/ben%2Fsync.sf_east:account/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection ben/sync.sf_east:account"

GET http://127.0.0.1:{{port}}/data/ben%2Fsync.sf_east%3Aaccount/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection ben/sync.sf_east:account"

GET http://127.0.0.1:{{port}}/data/sf_east:account/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: sf_east:account"

GET http://127.0.0.1:{{port}}/data/acme%2Fsalesforce.prod:account/list
Authorization: Bearer {{token}}
X-Project-Id: alice/shop
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: acme/salesforce.prod:account"

# --- First dot: acme/crm.foo.bar is an ordinary lake collection. ---

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/collection/acme%2Fcrm.foo.bar
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "foo.bar"
jsonpath "$.data.project" == "acme/crm"

GET http://127.0.0.1:{{port}}/data/acme%2Fcrm.foo.bar/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 0

# --- Listings: one row per address. Ordinary first. The refused custom account is absent. ---

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 5
jsonpath "$.data[0].name" == "foo.bar"
jsonpath "$.data[0].project" == "acme/crm"
jsonpath "$.data[0].owner" == "acme/crm"
jsonpath "$.data[0].integration" not exists
jsonpath "$.data[1].integration" == "sf_east"
jsonpath "$.data[1].name" == "account"
jsonpath "$.data[1].project" == "ben/sync"
jsonpath "$.data[1].owner" == "acme/salesforce"
jsonpath "$.data[1].label" == "Account"
jsonpath "$.data[1].label_plural" == "Accounts"
jsonpath "$.data[2].integration" == "sf_east"
jsonpath "$.data[2].name" == "invoice__c"
jsonpath "$.data[2].owner" == "ben/sync"
jsonpath "$.data[3].integration" == "sf_west"
jsonpath "$.data[3].name" == "account"
jsonpath "$.data[3].project" == "ben/sync"
jsonpath "$.data[4].integration" == "prod"
jsonpath "$.data[4].name" == "account"
jsonpath "$.data[4].project" == "acme/salesforce"
jsonpath "$.data[4].owner" == "acme/salesforce"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].integration" == "sf_east"
jsonpath "$.data[0].name" == "account"
jsonpath "$.data[0].project" == "ben/sync"
jsonpath "$.data[1].integration" == "sf_east"
jsonpath "$.data[1].name" == "invoice__c"
jsonpath "$.data[2].integration" == "sf_west"
jsonpath "$.data[2].name" == "account"

# --- Actions: the address resolves. Bad input is 400. No handler is 501. ---

GET http://127.0.0.1:{{port}}/actions/sf_east:set_owner
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.name" == "set_owner"
jsonpath "$.data.project" == "ben/sync"
jsonpath "$.data.owner" == "acme/salesforce"
jsonpath "$.data.integration" == "sf_east"
jsonpath "$.data.params" count == 1
jsonpath "$.data.params[0].name" == "owner_id"
jsonpath "$.data.params[0].required" == true

GET http://127.0.0.1:{{port}}/actions
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].integration" == "sf_east"
jsonpath "$.data[0].name" == "set_owner"
jsonpath "$.data[0].owner" == "acme/salesforce"
jsonpath "$.data[1].integration" == "sf_west"
jsonpath "$.data[1].name" == "set_owner"
jsonpath "$.data[1].project" == "ben/sync"
jsonpath "$.data[2].integration" == "prod"
jsonpath "$.data[2].name" == "set_owner"
jsonpath "$.data[2].project" == "acme/salesforce"

GET http://127.0.0.1:{{port}}/actions/set_owner
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "action not found: set_owner"

POST http://127.0.0.1:{{port}}/actions/sf_east:set_owner
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 400
[Asserts]
jsonpath "$.error" == "validation failed"
jsonpath "$.diagnostics[0].kind" == "required"

POST http://127.0.0.1:{{port}}/actions/sf_east:set_owner
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{"input": {"owner_id": "x"}}
HTTP 501
[Asserts]
jsonpath "$.error" == "no handler for action acme/salesforce.salesforce.set_owner"
jsonpath "$.data" not exists

# --- Queries resolve the same way. no_source is after auth. ---

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "east": {"collection": "sf_east:account"},
    "invoice": {"collection": "sf_east:invoice__c"},
    "dotted": {"collection": "acme/crm.foo.bar"},
    "plain": {"collection": "acme/salesforce.account"},
    "west_invoice": {"collection": "sf_west:invoice__c"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.east.records" not exists
jsonpath "$.data.east.diagnostics[0].kind" == "no_source"
jsonpath "$.data.east.diagnostics[0].message" == "no source for collection sf_east:account"
jsonpath "$.data.invoice.diagnostics[0].kind" == "no_source"
jsonpath "$.data.dotted.records" count == 0
jsonpath "$.data.dotted.error" not exists
jsonpath "$.data.plain.diagnostics[0].kind" == "unknown_collection"
jsonpath "$.data.west_invoice.diagnostics[0].kind" == "unknown_collection"

# --- Grants: sf_east:account opens that address. A bare account grant does not. ---

PUT http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["acme/salesforce@0.0.1-dev", "acme/crm@0.0.1-dev"], "public_permission_sets": ["east"]}
HTTP 200

GET http://127.0.0.1:{{port}}/data/sf_east:account/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 501
[Asserts]
jsonpath "$.error" == "no source for collection sf_east:account"

GET http://127.0.0.1:{{port}}/data/sf_east:account/fields
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data[0].name" == "status"

GET http://127.0.0.1:{{port}}/data/sf_west:account/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 403

GET http://127.0.0.1:{{port}}/data/account/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404

GET http://127.0.0.1:{{port}}/data/acme%2Fsalesforce.account/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404

GET http://127.0.0.1:{{port}}/data/missing/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown collection: missing"

GET http://127.0.0.1:{{port}}/actions
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data" count == 3

POST http://127.0.0.1:{{port}}/actions/sf_east:set_owner
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{"input": {"owner_id": "x"}}
HTTP 403

POST http://127.0.0.1:{{port}}/actions/missing
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 404
[Asserts]
jsonpath "$.error" == "action not found: missing"

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "east": {"collection": "sf_east:account"},
    "west": {"collection": "sf_west:account"},
    "plain": {"collection": "acme/salesforce.account"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.east.diagnostics[0].kind" == "no_source"
jsonpath "$.data.east.diagnostics[0].message" == "no source for collection sf_east:account"
jsonpath "$.data.west.diagnostics[0].kind" == "forbidden"
jsonpath "$.data.west.diagnostics[0].message" == "no read grant on sf_west:account"
jsonpath "$.data.plain.diagnostics[0].kind" == "unknown_collection"

PUT http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["acme/salesforce@0.0.1-dev", "acme/crm@0.0.1-dev"], "public_permission_sets": ["bare_account"]}
HTTP 200

GET http://127.0.0.1:{{port}}/data/sf_east:account/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 403

GET http://127.0.0.1:{{port}}/data/account/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404

GET http://127.0.0.1:{{port}}/actions
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 403

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{ "queries": { "east": { "collection": "sf_east:account" } } }
HTTP 200
[Asserts]
jsonpath "$.data.east.diagnostics[0].kind" == "forbidden"
jsonpath "$.data.east.diagnostics[0].message" == "no read grant on sf_east:account"

# --- The dependency draft later offers invoice__c. The address names two documents.
# sf_east:invoice__c is omitted. sf_west and prod gain the standard row, because
# they do not declare it as custom. A caller with no grant learns only that.
# A caller who may read gets 409. The action of the same string is missing. ---

# Naming collections replaces the list, so account is sent again. Actions
# are omitted, so set_owner stays.
PUT http://127.0.0.1:{{port}}/schema/acme/salesforce/0.0.1-dev/integration_type/salesforce
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "collections": [
    {
      "name": "account",
      "label": "Account",
      "label_plural": "Accounts",
      "fields": [{"name": "status", "type": "string", "label": "Status"}]
    },
    {"name": "invoice__c", "label": "Invoice", "label_plural": "Invoices"}
  ]
}
HTTP 200

GET http://127.0.0.1:{{port}}/data/sf_east:invoice__c/list
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 403

GET http://127.0.0.1:{{port}}/data/sf_east:invoice__c/list
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 409
[Asserts]
jsonpath "$.error" == "ambiguous address sf_east:invoice__c: the type offers it and the integration declares it"
jsonpath "$.data" not exists

GET http://127.0.0.1:{{port}}/data/sf_east:invoice__c/fields
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 403

GET http://127.0.0.1:{{port}}/data/sf_east:invoice__c/fields
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 409
[Asserts]
jsonpath "$.error" == "ambiguous address sf_east:invoice__c: the type offers it and the integration declares it"

GET http://127.0.0.1:{{port}}/actions/sf_east:invoice__c
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 404
[Asserts]
jsonpath "$.error" == "action not found: sf_east:invoice__c"

POST http://127.0.0.1:{{port}}/actions/sf_east:invoice__c
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{"input": {}}
HTTP 404
[Asserts]
jsonpath "$.error" == "action not found: sf_east:invoice__c"

GET http://127.0.0.1:{{port}}/actions/sf_east:set_owner
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
HTTP 200
[Asserts]
jsonpath "$.data.name" == "set_owner"

POST http://127.0.0.1:{{port}}/data/query
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{ "queries": { "invoice": { "collection": "sf_east:invoice__c" } } }
HTTP 200
[Asserts]
jsonpath "$.data.invoice.records" not exists
jsonpath "$.data.invoice.diagnostics[0].kind" == "forbidden"
jsonpath "$.data.invoice.diagnostics[0].message" == "no read grant on sf_east:invoice__c"

POST http://127.0.0.1:{{port}}/data/query
Authorization: Bearer {{token}}
X-Project-Id: ben/sync
X-Site-Id: dev
Content-Type: application/json
{
  "queries": {
    "invoice": {"collection": "sf_east:invoice__c"},
    "dotted": {"collection": "acme/crm.foo.bar"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.data.invoice.records" not exists
jsonpath "$.data.invoice.diagnostics[0].kind" == "ambiguous_address"
jsonpath "$.data.invoice.diagnostics[0].message" == "ambiguous address sf_east:invoice__c: the type offers it and the integration declares it"
jsonpath "$.data.dotted.records" count == 0
jsonpath "$.data.dotted.error" not exists

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 6
jsonpath "$.data[0].name" == "foo.bar"
jsonpath "$.data[1].integration" == "sf_east"
jsonpath "$.data[1].name" == "account"
jsonpath "$.data[2].integration" == "sf_west"
jsonpath "$.data[2].name" == "account"
jsonpath "$.data[3].integration" == "sf_west"
jsonpath "$.data[3].name" == "invoice__c"
jsonpath "$.data[4].integration" == "prod"
jsonpath "$.data[4].name" == "account"
jsonpath "$.data[5].integration" == "prod"
jsonpath "$.data[5].name" == "invoice__c"

GET http://127.0.0.1:{{port}}/schema/alice/shop/0.0.1-dev/collection/list
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].integration" == "sf_east"
jsonpath "$.data[0].name" == "account"
jsonpath "$.data[1].integration" == "sf_west"
jsonpath "$.data[1].name" == "account"
jsonpath "$.data[2].integration" == "sf_west"
jsonpath "$.data[2].name" == "invoice__c"

PUT http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/manifest
Authorization: Bearer {{token}}
Content-Type: application/json
{"dependencies": ["acme/salesforce@0.0.1-dev", "acme/crm@0.0.1-dev"], "public_permission_sets": ["east"]}
HTTP 400
[Asserts]
jsonpath "$.error" == "ambiguous address sf_east:invoice__c: type 'acme/salesforce.salesforce' offers collection 'invoice__c' and integration 'sf_east' declares it"

GET http://127.0.0.1:{{port}}/schema/ben/sync/0.0.1-dev/manifest
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data.public_permission_sets[0]" == "bare_account"

POST http://127.0.0.1:{{port}}/config/version/ben/sync
Authorization: Bearer {{token}}
Content-Type: application/json
{"version": "1.0.0", "from": "0.0.1-dev"}
HTTP 400
[Asserts]
jsonpath "$.error" == "ambiguous address sf_east:invoice__c: type 'acme/salesforce.salesforce' offers collection 'invoice__c' and integration 'sf_east' declares it"

GET http://127.0.0.1:{{port}}/schema/ben/sync/1.0.0/manifest
Authorization: Bearer {{token}}
HTTP 404
[Asserts]
jsonpath "$.error" == "unknown version: ben/sync@1.0.0"
