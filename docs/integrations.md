# Integrations

Target model. Not implemented. The decisions are the session with Ben on 2026-10-04, recorded in [#103](https://github.com/loco-hq/loco/issues/103). Declarations ([#100](https://github.com/loco-hq/loco/issues/100)), values ([#101](https://github.com/loco-hq/loco/issues/101)), actions ([#47](https://github.com/loco-hq/loco/issues/47), [#102](https://github.com/loco-hq/loco/issues/102)), and name resolution are merged; this document says how a connection uses them. The implementation issues at the end are not filed.

A package that wraps an outside system (BrickLink, Salesforce) serves its collections and actions live, with the installing dataset's credentials. An installer may connect more than once to the same kind of system. The connection is part of the name of the collection or action. There is no binding field and no tie-breaker.

## Vocabulary

| Term | What it is |
|---|---|
| **Integration type** | A kind of connection, declared in a package version: `loco/bricklink.bricklink`, `acme/salesforce.salesforce`. Code is registered against the owning project and the type name. First-party Rust, until the TypeScript engine in `FUTURE_IDEAS.md` exists. The production binary's registry starts empty, as the action registry does. |
| **Integration** | One connection of a type to one backend instance. Declared in a version (a name and a type), so a caller can address it and version copy carries it. Connected on a dataset: secret and variable values are set there. A published version does not record which backend a dataset reaches. `dev` can point at a sandbox and `prod` at the real org, on the same version. |
| **Free integration** | An integration the type's owner declares in its own namespace and marks mounted at the root of that namespace. Install `loco/bricklink`, set the free integration's values on your dataset, call `loco/bricklink.orders`. |
| **Installer integration** | An integration a depending project declares of a type it did not write (`sf_east`, `sf_west`), with its own values. |
| **Standard collection** | A collection the type offers (BrickLink orders, a Salesforce standard object). Fields belong to the type's owner. Every integration of the type exposes it; the address says which connection. |
| **Custom collection** | A collection one integration declares (a Salesforce custom object, `sf_east:invoice__c`), in the version of the project that owns that integration. Fields belong to that project. |
| **Connection** | The values and the HTTP client for the one integration a call was addressed to. Handlers and sources take this. They do not take the dataset's other credentials. |

An integration type declares:

- The secrets and variables every connection of this type needs. A variable may have a default. A secret has no default and no value. The list is inline on the type document. A consumer sees it and cannot add to it, the same rule as an action's `params`, and version copy copies the document.
- The standard collections it offers, and the actions it offers, as documents under the type. A consumer cannot add those either. A name with no `:` does not find them, except through the root mount.

## Live, not synced

Reads and writes on an integration's collections are calls to the outside system. v1 has no cache. A caching layer may sit in front of those calls later. It is not a second copy that `/data` reads instead.

BrickLink allows about 5,000 calls a day. A load of 50 orders is one list call plus one items call per order, about 51 calls. Repeating that on every page load spends the quota. That cost is the cache's job later. It is not a reason to sync in v1.

A public read grant on an integration collection spends that quota with the store's credentials: token-less `public` calls BrickLink as the store. A public `update` grant writes upstream. The permission set is still the consuming version's choice, as in [`identity.md`](identity.md).

Record ids are the upstream ids. In `GET /data/loco%2Fbricklink.orders/29471234` the id is the BrickLink order id. Lake collections keep the ids `Record::new_for_insert` mints.

## Names

**Rule: an unqualified name means the project that owns the running version. A dependency is written `{account}/{project}.{local}`. Inside `local`, `{integration}:{name}` is one connection's collection or action. A `local` with no `:` is an ordinary lake name, or the root-mounted free integration.**

```
address      = local | project "." local
project      = account "/" project-name
local        = name | integration ":" name
```

`account` and `project-name` are the project-id charset (`[a-z0-9_]`, starting with a letter or `_`). `integration` and `name` are the collection charset, `[a-z0-9_.-]+`, which is `collection_name_ok` in `http/version_schema.rs`. Both sides of `:` are non-empty. A second `:` does not resolve: `name` cannot contain `:`.

| Address | Project | Integration segment | Name |
|---|---|---|---|
| `sf_east:account` | self | `sf_east` | `account` |
| `ben/sync.sf_east:account` | `ben/sync` | `sf_east` | `account` |
| `sf_east:set_owner` | self | `sf_east` | `set_owner` |
| `loco/bricklink.orders` | `loco/bricklink` | none (root mount) | `orders` |
| `orders` | self | none | `orders` |
| `brocksbricks/orders.orders` | self, when that is the running project | none | `orders` |

`brocksbricks/orders.sf_east:account` is the same address as `sf_east:account` when the running project is `brocksbricks/orders`. A qualified name for self is the bare name, as elsewhere.

### Why `:`

`.` already separates the project from `local`. A project id contains no `.`, so the first `.` is that boundary, including when `local` itself contains a `.` (`foo.bar` is a legal collection name).

`split` (`split_qualified`) and grant matching (`collection_grant_matches`) both move to the first `.` for every name, not only an integration address. Today both take the last `.`, so `acme/crm.foo.bar` is read as project `acme/crm.foo` and is not found. The first `.` reads it as project `acme/crm` and name `foo.bar`. A name that resolves today has one `.` after the project id, so the first and the last are the same dot and the meaning does not change.

`:` is the integration separator because `collection_name_ok` does not allow it. An action param is checked by `param_name_ok`, which is the same charset and not `collection_name_ok`. A type's secret and variable names use that charset too, so a value row id has one `:`. Adding an integration cannot change what an existing name means: `orders` and `warehouse:orders` are different strings, and `orders` keeps the meaning it had before `warehouse` existed.

`/` is the project-id separator and the path separator. `.` cannot take the integration's job without a tie-breaker between `sf_east.account` (a dotted collection name, legal today) and an integration `sf_east` plus a collection `account`.

### Parse, then resolve

Parse is a property of the string. Resolve is a property of the version. Nothing walks a list of integrations looking for a collection named `account`.

1. If the string contains `/`, split on the first `.` into `project` and `local`. Otherwise `project` is self and `local` is the whole string. A project that is not self or a **direct** dependency is not found.
2. If `local` contains `:`, split on the first `:` into `integration` and `name`. Each side must pass `collection_name_ok`. Otherwise `local` is `name` and there is no integration segment.
3. With an integration segment: that project must declare an integration of that name, and `name` must be a collection or action document under its type, or a custom collection that integration declares. The type's project must be self or a direct dependency on the version that declares the integration.
4. With no integration segment, look up an ordinary collection or action on that project, at the ordinary paths. A standard collection or a type action is not on those paths, so this step does not find it, unless the project has a root-mounted integration and the type has a document of this name. Then the address is that connection. `acme/salesforce.account` is not found when `account` lives under the type and salesforce has no root mount. `sf_east:account` is step 3.

The root mount does not rewrite steps 1 or 2. It is how step 4 chooses a connection for a name that has no integration segment. The integration still has a name (`bricklink`). Collection and action addresses omit it. Value keys do not (below). A named integration never takes an existing spelling, because `:` is outside the charset. `warehouse:orders` is a new name even when `orders` already resolves.

Only the project that owns the type may mark an integration `root`, and only an integration of a type that project declares. An installer integration of `acme/salesforce.salesforce` cannot mount at `ben/sync`'s root: those addresses would be `ben/sync`'s own names, aimed at another package's collections. A version has at most one root-mounted integration. The root address has no integration segment left, so a second mount would need a tie-breaker. A second connection on the same package is a named integration (`sandbox:orders`).

### `/data` and `/actions`

The address is one path segment, percent-encoded the way a qualified collection already is. `/` is written `%2F`. `:` may be written as itself; `%3A` decodes to the same segment.

| Address | `GET` or `POST` |
|---|---|
| `loco/bricklink.orders` | `/data/loco%2Fbricklink.orders/list` |
| `loco/bricklink.orders` / `29471234` | `/data/loco%2Fbricklink.orders/29471234` |
| `loco/bricklink.set_status` | `/actions/loco%2Fbricklink.set_status` |
| `sf_east:account` | `/data/sf_east:account/list` |
| `sf_east:invoice__c` | `/data/sf_east:invoice__c/list` |
| `sf_east:set_owner` | `/actions/sf_east:set_owner` |
| `ben/sync.sf_east:account` | `/data/ben%2Fsync.sf_east:account/list` |

`GET /data/{collection}/fields` takes the same segment. The field list is the standard fields, or the custom collection's fields.

An unknown address is 404 (`unknown collection`, or `action not found: {name}`) before the write check, as today.

### Listings, query, and grants

`collection/list` and `GET /actions` today return each document's `project`, and the client builds `{project}.{name}`. One standard collection is now many addresses (`sf_east:account` and `sf_west:account`), and a root-mounted address hides the integration whose values the call uses. The row shape that carries that is an [open question](#open-questions). The addresses above are the strings a row has to be able to produce. A client does not guess which integration a name is bound to.

`POST /data/query` uses the same address as `"collection"`. `"sf_east:account"` and `"loco/bricklink.orders"` resolve through the parse above. A query that names a collection the caller may not read is that query's `forbidden` result, as in [`query.md`](query.md).

Field names do not gain a new rule. A bare field means the running project. A standard collection's fields belong to the type's owner, so from `brocksbricks/orders` the filter is `loco/bricklink.status`, not `status`. A custom collection's fields belong to the integration's owner, so from `ben/sync` the filter on `sf_east:invoice__c` is the bare `amount`. This is the conservative rule in [`query.md`](query.md) (open question 1 there, answered for v1).

A permission-set grant names the address the same way it names a collection today. `collection_grant_matches` compares the grant, after this parse, to the address the request resolved. The project in that comparison is the project the address is rooted in (who declared the integration, or who owns the ordinary collection), not the type owner that holds the fields.

```yaml
# ben/sync, a permission set
collections:
  - collection: sf_east:account
    read: true
  - collection: sf_east:invoice__c
    read: true
```

`sf_east:account` does not grant `sf_west:account`. A set `acme/salesforce` ships grants addresses in `acme/salesforce`'s namespace. It does not grant the installer's `sf_east:account`. The example package has no root mount, so a bare `account` grant does not resolve to the type's `account`. A consumer opts in by listing the set on the consuming version's manifest, as in [`identity.md`](identity.md). A bare grant is the set's own project. A qualified grant (`ben/sync.sf_east:account`) names that project exactly. Grant matching uses the first-`.` split above.

Members are unchanged. Token-less `public` still cannot run an action. A public data grant on an integration collection is a live call with the store's credentials; see [Live, not synced](#live-not-synced).

## Worked examples

### Brock's Bricks

`brocksbricks/orders` depends on `loco/bricklink@1.0.0`. Brock does not declare an integration. The free integration is `loco/bricklink`'s, mounted at that namespace's root. His dataset holds the values. A second BrickLink store, if he had one, would be his own integration of the same type (`warehouse`, addressed `warehouse:orders`), with a second set of values. This example is the one free connection.

The type. Secrets and variables are the connection's declarations. `base_url` is how a dataset points at the real API or at a fixture; the default is the BrickLink store API, so a store that sets nothing still has a host.

```yaml
# loco/bricklink  versions/1.0.0/integration_types/bricklink.yaml
label: BrickLink
secrets:
  - name: consumer_key
    label: Consumer key
    required: true
  - name: consumer_secret
    label: Consumer secret
    required: true
  - name: token_value
    label: Token
    required: true
  - name: token_secret
    label: Token secret
    required: true
variables:
  - name: base_url
    label: API base URL
    required: true
    default: "https://api.bricklink.com/api/store/v1"
```

The free integration. `type` is bare because the type is this project's. `root` is legal here and nowhere else.

```yaml
# loco/bricklink  versions/1.0.0/integrations/bricklink.yaml
label: BrickLink
type: bricklink
root: true
```

A standard collection. It lives under the type. A name with no `:` does not find it except through this root mount. The record id is the BrickLink order id, so the id is not also a field. `order_items` is the same kind of document, under the same type: `item_no`, `item_type`, `color_id`, `color_name`, `condition`, `qty`, `remarks`, `inventory_id`, and `order_id` (the parent order). Fields belong to the type's owner.

```yaml
# loco/bricklink  versions/1.0.0/integration_types/bricklink/collections/orders.yaml
label: Order
label_plural: Orders
```

```yaml
# loco/bricklink  versions/1.0.0/integration_types/bricklink/fields/orders/status.yaml
type: string
label: Status
```

The other order fields are `buyer` (string), `date` (string, the API's timestamp), and `totals` (string).

An action the type offers. Reading orders is the collection, not this action. [#104](https://github.com/loco-hq/loco/issues/104) does not have to ship `set_status`; the block is here so the free integration's action address is visible. Params are the action's, as today.

```yaml
# loco/bricklink  versions/1.0.0/integration_types/bricklink/actions/set_status.yaml
label: Set status
description: Sets one order's status. The order id is the BrickLink order id.
params:
  - name: order_id
    type: string
    label: Order
    required: true
  - name: status
    type: string
    label: Status
    required: true
```

The handler is registered against `(loco/bricklink, bricklink, set_status)`. Type actions register under the type. Ordinary actions keep `(project, name)`. The source is registered against `(loco/bricklink, bricklink)`. A run of `loco/bricklink.set_status` binds the free integration first, then calls that handler with that connection.

Dataset `dev` on `brocksbricks/orders`. The value name is the qualified integration, then `:`, then the declaration name. The integration's own name stays in the value key when the collection address drops it. Four secrets:

```
PUT /config/secret/brocksbricks/orders/dev/loco%2Fbricklink.bricklink:consumer_key
{"value": "…"}
```

and the same path with `consumer_secret`, `token_value`, and `token_secret`. A fixture dataset overrides the host. A dataset that does not call this route gets the default.

```
PUT /config/variable/brocksbricks/orders/dev/loco%2Fbricklink.bricklink:base_url
{"value": "http://127.0.0.1:9"}
```

`prod` on the same published version stores different values under the same names. The version stores none of them.

From a site pinned to `brocksbricks/orders`:

```
GET  /data/loco%2Fbricklink.orders/list
GET  /data/loco%2Fbricklink.orders/29471234
GET  /data/loco%2Fbricklink.order_items/list
POST /actions/loco%2Fbricklink.set_status
{"input": {"order_id": "29471234", "status": "PENDING"}}
```

A hosted bundle omits `X-Project-Id` and `X-Site-Id`. Any other caller sends them, as for `/data` today.

A query from that site names the type owner's field:

```json
{
  "queries": {
    "pending": {
      "collection": "loco/bricklink.orders",
      "where": { "field": "loco/bricklink.status", "op": "eq", "value": "PENDING" }
    }
  }
}
```

Whether BrickLink's source declares that it can honor `eq` on `status` is the capability list on the source. A filter it does not declare is an error result for that query, not a broader list.

### Two Salesforce orgs

`ben/sync` depends on `acme/salesforce@1.0.0` and declares two integrations. The type is the package's. The instance URL is a variable with no default: each org has its own, and the version must not hardcode either.

```yaml
# acme/salesforce  versions/1.0.0/integration_types/salesforce.yaml
label: Salesforce
secrets:
  - name: client_id
    label: Client id
    required: true
  - name: client_secret
    label: Client secret
    required: true
  - name: refresh_token
    label: Refresh token
    required: true
variables:
  - name: instance_url
    label: Instance URL
    required: true
```

```yaml
# ben/sync  versions/1.0.0/integrations/sf_east.yaml
label: Salesforce East
type: acme/salesforce.salesforce
```

```yaml
# ben/sync  versions/1.0.0/integrations/sf_west.yaml
label: Salesforce West
type: acme/salesforce.salesforce
```

Neither sets `root`. Standard `account` lives under the type and is addressed twice, as `sf_east:account` and `sf_west:account`. `acme/salesforce.account` has no `:` and no root mount, so it is not found. Fields of `account` belong to `acme/salesforce`.

```yaml
# acme/salesforce  versions/1.0.0/integration_types/salesforce/collections/account.yaml
label: Account
label_plural: Accounts
```

East has a custom object. It is a collection document under the integration, not a collection of the type, and not an ordinary collection named `invoice__c` (that name, bare, is a different address and a different document).

```yaml
# ben/sync  versions/1.0.0/integrations/sf_east/collections/invoice__c.yaml
label: Invoice
label_plural: Invoices
```

```yaml
# ben/sync  versions/1.0.0/integrations/sf_east/fields/invoice__c/amount.yaml
type: string
label: Amount
```

`sf_west:invoice__c` does not resolve. West did not declare it. The type does not offer it.

Values on dataset `dev`, two connections, same secret names, different rows:

```
PUT /config/secret/ben/sync/dev/sf_east:client_id
PUT /config/secret/ben/sync/dev/sf_west:client_id
```

and the same for `client_secret` and `refresh_token`. `instance_url` is the variable:

```
PUT /config/variable/ben/sync/dev/sf_east:instance_url
PUT /config/variable/ben/sync/dev/sf_west:instance_url
```

From `ben/sync` the integration is bare. A caller in another project writes `ben%2Fsync.sf_east:client_id`.

Compare the two orgs in one batch. Each query is authorized on its own. They are not one snapshot: one source call is not the lake's read transaction.

```json
{
  "queries": {
    "east": {
      "collection": "sf_east:account",
      "where": { "field": "acme/salesforce.name", "op": "eq", "value": "Ada" }
    },
    "west": {
      "collection": "sf_west:account",
      "where": { "field": "acme/salesforce.name", "op": "eq", "value": "Ada" }
    }
  }
}
```

```yaml
# acme/salesforce  versions/1.0.0/integration_types/salesforce/actions/set_owner.yaml
label: Set owner
description: Sets the owner of one Account. Runs against the integration in the address.
params:
  - name: account_id
    type: string
    label: Account
    required: true
  - name: owner_id
    type: string
    label: Owner
    required: true
```

```
GET  /data/sf_east:account/list
GET  /data/sf_west:account/list
GET  /data/sf_east:invoice__c/list
POST /actions/sf_east:set_owner
{"input": {"account_id": "001…", "owner_id": "005…"}}
```

`set_owner` is registered against `(acme/salesforce, salesforce, set_owner)` and runs with East's connection only. It cannot read West's secrets, and West's source is not part of the call. Moving a record from one org to the other is two requests from the caller: a read of one address and a write of the other. An ordinary action on `ben/sync` is not called for either integration, so it does not receive either connection.

## Declarations and values

| Document | Where | Who owns the fields or the list |
|---|---|---|
| Integration type | `${project}/versions/${version}/integration_types/${name}` | The package. Secrets and variables are inline. |
| Standard collection, its fields, its actions | `.../integration_types/${type}/collections/${name}`, `.../fields/${collection}/${name}`, `.../actions/${name}` | The type's owner. A name with no `:` does not find these, except through the root mount. |
| Integration | `${project}/versions/${version}/integrations/${name}` | The project that connected. `type` is bare for a type this project declares, qualified for a dependency's. |
| Custom collection and its fields | `.../integrations/${integration}/collections/${name}` and `.../integrations/${integration}/fields/${collection}/${name}` | The integration's owner. A bare `invoice__c` does not find this document. |

Each type has its own directory, so two types in one project may both offer `orders`. The files do not collide, and a name with no `:` still does not find either, unless one of them is the root mount.

Hard-coded path segments stay plural. `project` and `version` are template variables and stay out of the YAML body, as elsewhere.

Writes are `/schema` and draft-only. Version copy copies the type directory (the type document, its collections, fields, and actions) and each integration directory (the integration and its custom collections and fields), along with the ordinary collections and actions it already copies. It does not copy datasets, records, or values.

In a version with a root mount, a name that has a collection or action document under that type may not equal an ordinary collection or action name. The check runs on both writes: saving the ordinary document, and saving the type's document or setting `root`. A second type in the same version is unaffected, because its documents are not on the ordinary paths and bare `orders` is the root mount's name only.

An installer cannot declare a custom collection on a dependency's free integration. They declare their own integration, and the custom collection lives under that.

Deleting an integration on a draft cascades its custom collections and fields (`delete_by_prefix`). A grant that names `sf_east:account` goes inert, the same as a grant on a deleted collection. Value rows stay. `DELETE` still removes a row without looking the declaration up. The value key carries neither version nor type, so re-declaring `sf_east` with another type reuses any rows whose declaration names match.

Dynamic discovery is later. A type may offer a describe hook that **proposes** collection and field declarations into a draft. Someone publishes the draft. The remote system does not change a published version, and it does not change what a name resolves to on read. Until that hook exists, a custom object is written by hand, as `invoice__c` is above.

### Secrets and variables

Connection credentials are declared on the type, not as loose secret and variable documents on the version. The declaration fields are the ones [#100](https://github.com/loco-hq/loco/issues/100) defined. A secret has `label`, `description`, and `required`, and a body that carries `default` or `value` is a 400. A variable may also have `default` (a string; empty means none). A secret and a variable of one name on one type are a 400. Whether a version may still declare a secret that is not a connection is an [open question](#open-questions).

Values stay on the dataset in the [#101](https://github.com/loco-hq/loco/issues/101) storage (`SecretStore`, `$secrets`, `$variables`). Storage, gates, and purge order are unchanged. A write is allowed when any version of the project declares the name, itself or through a direct dependency (`http/config_values.rs`). Brock's `loco/bricklink.bricklink:consumer_key` is the dependency half of that rule. The list is still the declarations of the versions this dataset's sites pin; a dataset no site pins lists nothing. Version copy still does not copy values.

The row id changes. It is the qualified integration, then `:`, then the declaration's bare name.

| Integration, from the project storing the value | Secret `consumer_key` is stored as |
|---|---|
| This project's `sf_east` | `sf_east:consumer_key` |
| This project's `sf_east`, written qualified (`ben/sync.sf_east:…`) | `sf_east:consumer_key` |
| A dependency's free integration | `loco/bricklink.bricklink:consumer_key` |

`:` is free here for the same reason it is free in a collection name. The canonical name is what `secret_aad` already binds, so two integrations' `consumer_key` rows do not share ciphertext. `DELETE` still skips declaration lookup: it strips a `{project}.` prefix and uses the rest as the id, `:` included. A qualified name for this project's own integration is the bare integration name, as a qualified name for this project's own secret is the bare secret today.

Handlers and sources read values only through the integration the address resolved to. `secret("consumer_key")` is that integration's row on the request's dataset. It is not resolved through `split`. A secret the installer declared under the same bare name, on the version or on another integration, is a different row. `Ok(None)` means this integration's type declares it and this dataset has not set it. A stored variable wins, including `""`. With no row, a non-empty default is returned. Asking for a name the type does not declare is a bug in the handler and is 500.

Before a source call or an integration action runs, every required secret and variable of **this** integration must be set on the dataset, or the call is 400 `required configuration is not set`, one diagnostic per missing name. The check is this integration's declarations only. It does not also require the owning project's loose declarations. Whether those loose declarations still exist, and whether an ordinary action checks them, is [open question 1](#open-questions). A non-empty variable default counts as set. The check uses `list` and does not decrypt, so an empty stored secret counts as set and the 400 does not contain plaintext. That is the [#102](https://github.com/loco-hq/loco/issues/102) check, scoped to the one integration.

The HTTP client on the connection is the process-wide client actions already use: 5s to connect, 30s for the call, no proxy, same-host redirects, at most ten.

## CollectionSource

`DataAdapter` (`loco-lake`, `adapter.rs`) is synchronous, has a method for every verb, and takes no connection. `server.rs` holds one adapter for every collection. An integration collection needs a different trait: the call is HTTP, the credentials differ per integration, and an upstream API does not implement the whole of [`query.md`](query.md).

The replacement is an async trait, working name `CollectionSource`. `/data` and `/data/query` call it for every collection, lake included. The lake's implementation is today's adapter behind the trait. Those methods do not await while holding the adapter lock. An integration implementation awaits the HTTP client on the request task, as an action handler does.

Each call receives the connection: the dataset id, the qualified integration, `secret` / `variable` as above, and the HTTP client. The lake implementation has no integration and does not read secrets. `delete_dataset` is not a collection verb. It stays on the lake adapter. A source has no dataset to purge.

The source declares capabilities:

- Which verbs it supports: get, list, insert, update, delete, query.
- Which `/data/query` operations it can honor: the comparison ops (`eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `in`, `exists`), the combinators (`and`, `or`, `not`), and order (`asc` / `desc`, collation `binary` / `natural`), including `limit` and `cursor`.

The lake declares the full set. Anything else is an error in the diagnostic shape `/data` already returns, on that query inside a batch or on the verb's request. It is not an empty result, a dropped condition, or a truncated page reported as the last page (`cursor: null` means the source said there is nothing more). A source that cannot return every row does not support `list`. The caller uses `query` with a filter the source declares. `fields` projection stays in front of the source, as it stays in front of the lake today: the source returns the record, and the projection does not have to be a capability.

Update is a patch of the upstream fields. The source does not replace the record's fields wholesale.

A query batch stays one response of named results. Queries for one lake collection still share the lake's snapshot. A query aimed at an integration runs on that source. The batch does not claim those two are one snapshot.

```text
/data  and  /data/query
  parse the address, authorize, reject undeclared capabilities
  require this integration's secrets and variables
  read:
      CollectionSource, with this connection          live
      or the cache, later, in that same place
      then merge the sidecar row                      #117, not built
           key: integration-qualified name + upstream id
                loco/bricklink.bricklink:orders
                sf_east:account
  write:
      upstream fields   →  the source, as a patch
      installer fields  →  the sidecar row            #117, not built
```

The sidecar and the cache are not part of v1. The trait's update takes upstream fields only, and the read path merges after the source returns, so both can be added without a second route and without the source knowing about them.

[#117](https://github.com/loco-hq/loco/issues/117). An installer may later add a field to a dependency's collection. The owner's fields stay keyed bare in `fields`. The added field is keyed qualified (`brocksbricks/orders.picked_by`). BrickLink has nowhere to store `picked_by`. The added fields are a lake row keyed by the integration-qualified collection name and the upstream id, merged on read. The collection half of that key is the integration's own name, root mount included: `loco/bricklink.bricklink:orders`, not the root address `loco/bricklink.orders`, and `sf_east:account`, not `acme/salesforce.account`. Two connections that share an upstream id do not share added fields. The key cannot collide with an ordinary lake key `{owner}.{name}`, because `:` is outside the collection charset. A patch that names both an upstream field and an added field writes each half in its own place. A handler that replaces `fields` wholesale would drop the added half; the source never receives it. Building that merge is #117, not the issues below.

The cache, when it exists, sits in front of the source and behind the merge. A hit is still an upstream record, then the sidecar is applied. Callers do not choose cached versus live. v1 always takes the live branch.

[#120](https://github.com/loco-hq/loco/issues/120). A source receives the connection, not `Arc<dyn DataAdapter>` and not every secret on the dataset. Action handlers still receive the site's `VersionSchema` and the raw adapter, which is the reach #120 exists to narrow. This design does not replace that context. It does not give the source a wider one. Sandboxing waits for #120, and for the TypeScript engine. Until then the registry is Rust we review.

System fields on a live record (`$created_at`, `$owner`, and the rest) are an [open question](#open-questions). `$id` is the upstream id either way.

## How #100, #101, and #104 change

[#100](https://github.com/loco-hq/loco/issues/100) and [#101](https://github.com/loco-hq/loco/issues/101) are merged. They are not reopened. The follow-up is an issue below. What changes:

- A credential that belongs to a connection is declared on the integration type (inline list: `label`, `description`, `required`, and `default` on a variable only). It is not a loose `${version}/secrets/${name}` document.
- The value row id is `{qualified integration}:{name}` in the existing `$secrets` / `$variables` collections. `SecretStore`'s trait stays `put` / `get` / `delete` / `list` / `delete_dataset` of a dataset id and a name string. The name string grows the integration.
- A handler or a source reads only the integration the call was addressed to. An integration action's required-config check is that integration's declarations only. Loose declarations are [open question 1](#open-questions).
- The list a client sees is one row per integration per declaration. The columns beyond today's `name`, `project`, `set`, and (for a variable) `value` and `source` are the listing question below.

Storage, gates, and purge order are unchanged ([#101](https://github.com/loco-hq/loco/issues/101)).

[#104](https://github.com/loco-hq/loco/issues/104) stays the BrickLink package. It is edited in place when this document is accepted. It does not stay a sync into the lake.

- Seed package `loco/bricklink`, published version `1.0.0`: the integration type, the free integration mounted at the root, and live collections `orders` and `order_items` with their fields and the example action under `integration_types/bricklink/`. A bare `orders` on that version is the root mount. An ordinary collection of the same name is a 400.
- The four OAuth secrets, required, and `base_url` defaulting to the store API. CI sets `base_url` on the fixture dataset. No live BrickLink call in CI.
- The OAuth 1.0a signer is unit-tested against a known-good signature.
- A store that depends on `loco/bricklink@1.0.0` and has the four secrets set reads `loco%2Fbricklink.orders` and `loco%2Fbricklink.order_items` through `/data` and `/data/query`. There is no `sync_orders`, and no "skip item calls for unchanged orders," because nothing is cached.
- A bad credential is 502 with the upstream message, not 500. The message must not echo the request.
- The record id is the upstream id.
- The rate limit is accepted. The cache is later.

`set_status` in the example is not an acceptance criterion of #104.

[#108](https://github.com/loco-hq/loco/issues/108) is unchanged and unsolved. Reads see direct dependencies only. `brocksbricks/orders` must depend on `loco/bricklink` to resolve `loco/bricklink.orders` and to store `loco/bricklink.bricklink:consumer_key`. A store that depends only on some `brickos/pulling` that depends on BrickLink cannot see the integration, list its secrets, or set them. Transitive visibility, or a package re-declaring the connection its dependency needs, is #108. Not while the store depends on `loco/bricklink` directly.

## Issues to file

Not filed here. File these from the merged document. One shippable change each. #104 is an edit of the existing issue, not a fifth one. #117, #120, the cache, and the describe hook are not in this list.

1. **Add integration type and integration declarations on a version.** The documents and paths above, inline secrets and variables, standard collections, fields, and actions under the type, custom collections and their fields under the integration, the root-mount uniqueness 400 (checked on both writes), draft delete of an integration, version copy of the declarations. The source registry is keyed by owning project and type name. Type actions register under the type, `(project, type, name)`; ordinary actions keep `(project, name)`. Both start empty in the production binary. Blocked by nothing in this list. The loose-secret question is settled in issue 4, not by deleting the #100 types in this issue.

2. **Resolve integration addresses.** The parse in `split` and in grant matching, both on the first `.`, one path segment on `/data` and `/actions`, `/data/query`, and grant matching against the address. A name with no `:` finds a standard collection only through the root mount. Listing rows are the open question; close it in this issue. The recommendation below is the proposal. Depends on issue 1.

3. **Route `/data` through `CollectionSource`.** The async trait, the connection argument, capability declaration, the lake implementation, upstream ids, and an error (never a widened result) for a verb or a filter the source does not declare. `/data` and `/data/query` call the trait. The read and write path keeps the seam in the diagram: sidecar merge and cache are not implemented. Depends on issues 1 and 2.

4. **Key connection secrets and variables by integration.** Declarations on the type, row ids in the existing `SecretStore` and `$variables`, the required check and `secret()` / `variable()` scoped to the integration the call was addressed to. Depends on issue 1. This issue also settles whether loose version-level declarations remain. The recommendation is to keep them.

#104 waits until these four are in. It is the first registered source.

## What this replaces

| Today | After |
|---|---|
| One synchronous `DataAdapter`. Every verb is assumed. No per-call credentials. | `CollectionSource`. Async. Capabilities. A connection on each call. The lake implements the trait, so `/data` has one interface. |
| Records are lake rows with minted ids. | An integration collection's records are the upstream system's. The id is the upstream id. |
| Secrets and variables are declared on the version. One value per name per dataset. | A connection's secrets and variables are declared on the integration type. One value per integration per dataset. |
| A handler reads the owning package's secrets on the installing dataset. | A handler or a source reads the secrets of the integration it was called for, on that dataset. |
| [#104](https://github.com/loco-hq/loco/issues/104) `sync_orders` writes a cache, then `/data` reads the cache. | `/data` reads BrickLink. `sync_orders` is not how orders are read. |
| A name is `{project}.{name}`. | A name may be `{project}.{integration}:{name}`. `{project}.{name}` remains an ordinary collection, or the root-mounted free integration. |

## Non-goals

- The cache, the describe hook, sidecar merge (#117), transitive dependencies (#108), and handler sandboxing (#120). The seams are above so those can land later.
- A workflow language, a trigger, or a schedule. An action is still a handler.
- Writing upstream records into the lake as the way to read them.
- A caller-selected adapter. The address selects the source.
- Registering anything but Rust. The TypeScript engine is not this design.
- Changing lake-collection ids, `/data` authorization, or the HTTP client's timeouts, proxy refusal, and redirect limit.
- `loco-client`, Studio, and MCP. They call the addresses. They do not define them.
- Record-level security, and a transaction around a source call. The lake has none; an upstream call is whatever that system did.

## Later

- **Cache.** In front of the source, behind the sidecar merge. BrickLink's daily budget is the case that wants it. v1 spends the budget.
- **Describe hook.** Proposes declarations into a draft. The schema stays explicit and versioned.
- **Transitive dependencies ([#108](https://github.com/loco-hq/loco/issues/108)).** A store whose dependency is indirect cannot name `loco/bricklink.orders` or set that integration's values.
- **Sandboxing ([#120](https://github.com/loco-hq/loco/issues/120)).** A package-scoped schema view and a validated data handle, before any handler that is not first-party Rust. The connection is already one integration's values and the shared HTTP client.

## Open questions

1. **Loose version-level secrets and variables.** Connection credentials move onto the type. [#100](https://github.com/loco-hq/loco/issues/100)'s documents (`${version}/secrets/${name}`, `${version}/variables/${name}`) are the other place a version can declare a name. The session did not say whether that place remains for a setting that is not a connection (a license key, a flag, an ordinary action's credential).

   - **Keep them** for non-connections. An integration source and an integration action do not read them; they read the integration they were called for, so a loose `consumer_key` is not a store's token. An ordinary action keeps the [#102](https://github.com/loco-hq/loco/issues/102) behavior. Two declaration sites, and a package can put a token in the wrong one. The required check then reports the integration's secret as unset.
   - **Remove them.** Every secret belongs to an integration type. A package with one credential declares a type and one free integration. One place to look. #100's paths go away. There is no production data on them yet. A setting that is not a connection has to be modeled as a connection.
   - **Freeze them.** Existing documents still load. New work does not read them. Two shapes in the binary, one of them dead.

   Recommendation: **keep them.** "Integration" stays a connection to a backend. A license key is not one. The requirement the session actually sets is that two Salesforce orgs do not share a value, which the per-integration row id does. Issue 4 closes this.

2. **Listing shape.** `collection/list`, `GET /actions`, and the secret and variable value lists. Decided: each address in this document is representable, a standard collection expands to one row per integration that exposes it, and a root-mounted row still tells the client which integration's values the call uses. Not decided: the columns.

   - **Expand `name`.** `name` becomes the local address (`sf_east:account`, or `orders` for a root mount). `project` stays. Today's client builds `{project}.{name}` and the result is the address. A root-mounted row does not say that the values live on `bricklink`. A standard collection's field owner (`acme/salesforce`) and the address's project (`ben/sync`) are different, and one `project` field cannot be both.
   - **Join on the client.** `collection/list` stays the schema documents. A new integration list carries type, root, and custom collection names. The client crosses them. Closer to the stored documents. Every UI rebuilds the same cross, and `GET /actions` stops being the list a hosted page can render directly.
   - **One row per address, with the integration beside the name.** Recommended columns:

| Field | Meaning |
|---|---|
| `project` | The project the address is rooted in. |
| `integration` | Integration name. Absent on an ordinary lake collection. Present on a root-mounted row, even though the address omits it. |
| `name` | Collection or action name only (`orders`, `account`, `invoice__c`). |
| `root` | True when this integration is mounted at the root. |
| `owner` | Who owns the fields or the action params. The type's project, or the integration's project for a custom collection. |

The address is `reference(project, name)` when `root` is set or `integration` is absent, and `reference(project, integration + ":" + name)` otherwise. `reference` is today's rule: bare for the running project, `{project}.{local}` for a dependency.

Value rows keep `name`, `project`, `set`, `updated_at`, and the variable's `value` and `source`. They add `integration`, the canonical qualified integration (`sf_east`, or `loco/bricklink.bricklink`), so two `consumer_key` rows are distinct. `name` stays the declaration name.

Recommendation: **one row per address**, columns as in the table. `name` keeps one meaning. Issue 2 closes the collection and action lists. Issue 4 closes the value list, using the same `integration` column.

3. **System fields on a live record.** `$id` is the upstream id. [`query.md`](query.md) also has `$created_at`, `$created_by`, `$updated_at`, `$updated_by`, and `$owner`, and the lake fills them. An upstream record does not have those.

   - **Omit them.** The source fills `$id` only. A timestamp the API returns is a declared field (`date` on a BrickLink order). A query that filters or orders by a system field the source does not declare is a capability error.
   - **Invent them.** `$created_at` becomes the upstream's created time when one exists, and `$owner` becomes the caller. Two clocks, and a missing upstream timestamp has to be something.

   Recommendation: **omit them.** The lake source keeps declaring the full set. A sidecar row's own lake timestamps are not the upstream record's, and v1 has no sidecar row to confuse them with. Issue 3 closes this.
