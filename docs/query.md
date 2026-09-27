# Query

Target model. Not implemented. Supersedes the query shape sketched in #15.

A **query** reads records from one collection in the lake: which records (`where`), in what
order (`order`), how many (`limit`, `cursor`), and which fields (`fields`). A **batch** is a
set of named queries sent in one request.

This document covers reads only. Writes — patches, inserts, ordered operations, transactions
— are a separate API with its own design ([Writes are separate](#writes-are-separate)).

## Decisions

| # | Decision | Short reason |
|---|---|---|
| 1 | The wire format is a JSON query tree we define, not GROQ or another text language. | Validated before it runs, authorized per collection, compiled to SQL. |
| 2 | Reads only. Writes get their own endpoint and their own document. | Reads are idempotent and independent; writes need ordering and atomicity. |
| 3 | Queries are batched by name. Each query succeeds or fails on its own. A batch reads one snapshot. | Fewer round trips; one bad query doesn't sink the page. |
| 4 | Collection and field names are fully qualified, or bare and resolved to the running project. | The #28 rule, applied from day one. |
| 5 | Pagination is by opaque cursor, not offset. | Stable under concurrent inserts; changing later breaks clients. |
| 6 | Problems are reported as `Diagnostic`s (`validation.rs`), extended to point at a query, record, and field. | One error shape for reads now and writes later. |

## 1. A JSON query tree, not GROQ

GROQ has an open spec and is pleasant to write. It was considered and rejected for now:

- **Its power is joins and projections over references.** `->` dereferencing across a graph of
  documents is what GROQ is for. Loco has no references, and `Value` has no `Array` or
  `Object` yet (#18). We would be implementing a language whose best parts have nothing to
  work on.
- **It is a large implementation.** Parser, evaluator, null-propagation semantics, function
  library, pipelines. There is no mature Rust implementation to adopt, and compiling GROQ to
  SQL for `SqliteAdapter` is hard enough that Sanity runs its own backend for it.
- **It fights our authorization model.** `*[...]` ranges over every document and each `->`
  crosses collections. Permission sets grant verbs per collection. With a string we must parse
  before we can even say which collections a query touches.
- **It swallows errors.** GROQ evaluates a type mismatch or a missing field to `null`. We want
  to say "unknown field `agee` on `pet`", with a path.
- **Sanity itself splits reads from writes.** GROQ is read-only; mutations are a separate API.
  That is Decision 2.

Other options, briefly:

- **SQL** in the public API. Ruled out by #15, and it leaks the storage layer.
- **GraphQL** generated per version. Batching and error paths come built in, but it means
  generating and serving a schema per pinned version and a second resolver layer. Too heavy for
  what we need now.
- **Mongo / Directus / Firestore-style JSON filters.** This is the family the tree below is in.
  We borrow the shape, not a spec: none of them is a standard we'd gain from conforming to.

What the JSON tree gives us:

- **Checked before it runs.** Every collection and field name resolves against the running
  `VersionSchema` first. All problems are collected, not just the first.
- **Authorization is a lookup.** Each query names exactly one collection, so the read check is
  one grant lookup per query. Row-level policy, when it exists, becomes extra `where` clauses
  the server adds to the tree.
- **Portable.** Adapters compile the same resolved tree: to SQL (`json_extract` over the
  `fields` column) for sqlite, to a predicate for memory.
- **Agent-ready.** A JSON Schema for the tree is an MCP tool definition and an SDK type.
- **A text syntax can come later.** A GROQ-like string could compile into this same tree. The
  server never depends on it.

## 2. Writes are separate

This API does not write. Reads are idempotent and safe to retry, and each query in a batch
stands alone. Writes need what reads do not:

- **Order.** An update after an insert must see the insert.
- **References between operations.** Insert a batch, then insert its lots with the new id.
- **Atomicity.** Whether a failing operation rolls back the others.
- **Different authorization.** create / update / delete, not read.

A later document designs mutations, probably an ordered list of operations applied
all-or-nothing, in the spirit of Sanity's mutations API. It reuses the `Diagnostic` shape from
[Decision 6](#6-diagnostics), which is where per-record and per-field validation errors, and
later custom record- and field-level checks, belong.

## 3. Batches

`POST /data/query`, site-scoped by `X-Project-Id` / `X-Site-Id` like the rest of `/data`.

```json
{
  "queries": {
    "batch": {
      "collection": "batch",
      "where": { "field": "$id", "op": "eq", "value": "3f0c…" }
    },
    "lots": {
      "collection": "lot",
      "where": { "and": [
        { "field": "batch_id", "op": "eq", "value": "3f0c…" },
        { "field": "qty", "op": "gt", "value": 0 }
      ] },
      "fields": ["item_no", "color_code", "condition", "qty"],
      "order": [{ "field": "item_no" }, { "field": "color_code" }],
      "limit": 100
    },
    "colors": { "collection": "color", "fields": ["code", "label"], "limit": 500 }
  }
}
```

This is the BrickOS batch page in one request. Today it lists every lot and filters by
`batch_id` in the browser.

- **Named.** Keys are client-chosen names (`[a-z_][a-z0-9_]*`). Results come back under the
  same keys. Names, not positions, so a diagnostic can say which query it's about.
- **Independent.** A query that fails validation or authorization gets an error result. The
  others still run. The HTTP status is 200 when the batch itself was well-formed; it is 4xx only
  when the body can't be parsed as a batch at all.
- **One snapshot.** Every query in a batch reads the same state: one read transaction in sqlite,
  one read lock in memory.
- **Bounded.** A maximum number of queries per batch (initially 20) and a maximum `limit` per
  query (initially 500; default 50).
- **Queries don't reference each other** in v1. "The lots of the batch the first query found"
  is a join, and joins wait for references.

`GET /data/{collection}/list` with no parameters is unchanged and still returns every record.

### Response

```json
{
  "ok": true,
  "data": {
    "batch":  { "records": [ … ], "cursor": null },
    "lots":   { "records": [ … ], "cursor": "eyJr…" },
    "colors": { "error": "query failed", "diagnostics": [
      { "severity": "error", "kind": "forbidden", "path": "colors", "message": "no read grant on brickos/inventory.color" }
    ] }
  },
  "diagnostics": [
    { "severity": "warning", "kind": "type_mismatch", "path": "lots/8a1e…/qty", "message": "expected integer, got string" }
  ]
}
```

Records keep the lake shape (`id`, `created_at`, …, `fields`). With `fields`, the `fields` map
holds only the named fields; system fields are always present.

## 4. Names

**Rule: an unqualified name means the project that owns the running version. A dependency's
collection or field must be written fully qualified: `{user}/{project}.{name}`.**

This is the rule in `CLAUDE.md` and #28. The query layer implements it strictly from the start.
It must not call today's `VersionSchema::collection` / `field`, which fall through to
dependencies in manifest order.

- `"collection": "lot"` → `brickos/inventory.lot`, when the running version is a
  `brickos/inventory` version.
- `"collection": "acme/crm.contact"` → `contact` from dependency `acme/crm`. Must be a **direct**
  dependency in the manifest, or it's `unknown_collection`.
- A qualified name for the running project itself (`brickos/inventory.lot`) is accepted and is
  the same as the bare name.

Qualified collection names are already the lake's key: `collection_key` (`http/paths.rs`) stores
records under `{owner_project}.{name}`. Resolution produces that key directly.

Fields follow the same rule: `"field": "qty"` is the running project's `qty` field;
`"field": "acme/crm.email"` is `acme/crm`'s. See [Open question 1](#open-questions) for what
this means when the collection itself comes from a dependency.

### System fields

Record metadata is not a schema field. It is addressed with a `$` prefix so it can never
collide with a user field: `$id`, `$created_at`, `$created_by`, `$updated_at`, `$updated_by`,
`$owner`. They can be used in `where` and `order`. They are always returned, so they don't go in
`fields`.

## 5. Filters, order, cursors

### `where`

A condition is either a comparison or a combinator.

```
comparison  = { "field": name, "op": op, "value": scalar | [scalar] }
            | { "field": name, "op": "exists", "value": true | false }
combinator  = { "and": [condition, …] } | { "or": [condition, …] } | { "not": condition }
```

| op | value | matches when the field is… |
|---|---|---|
| `eq` / `ne` | scalar | equal / not equal |
| `lt` `lte` `gt` `gte` | number or string | ordered below / above (strings compare as bytes; timestamps are RFC 3339 so this orders them) |
| `in` | non-empty list of scalars | equal to one of them |
| `exists` | `true` / `false` | present and not null / absent or null |

- **Missing is null.** The lake is schemaless, so a record can lack a field. For every op
  except `exists`, a missing field behaves as `null`.
- **Null only matches null.** `eq null` matches null or missing. Ordering ops never match null.
- **Values are type-checked against the field**, up front: `qty gt "3"` is a `type_mismatch`
  error on the query, not an empty result. A record whose stored value has drifted to another
  type doesn't match (and gets a read warning if returned by some other condition).
- Depth and size are bounded (initially depth 8, 100 comparisons).
- No text search, no regex, no list ops in v1. List ops arrive with #18.

### `order`

A list of `{ "field": name, "dir": "asc" | "desc" }`, `dir` defaulting to `asc`. `$id` is always
appended as the final key so every order is total, which is what makes a cursor possible.
Default order is `[$created_at, $id]`. Nulls sort first ascending.

### `limit` and `cursor`

The response carries `cursor`: `null` when there is nothing more, otherwise an opaque string.
Send it back as `"cursor"` on the same query to get the next page.

- A cursor encodes the order-key values of the last record returned (keyset pagination), not an
  offset. Inserts and deletes between pages don't skip or repeat records.
- It also encodes a hash of the query's `collection`, `where`, and `order`. A cursor sent with a
  different query is a `cursor_mismatch` error. `limit` and `fields` may change between pages.
- It is opaque and unsigned. Tampering with one can only produce a different page of a query
  the caller is already allowed to run.
- No total count in v1: a count means a second scan.

## 6. Diagnostics

`validation.rs` already has the shape: `Diagnostic { severity, kind, path, message }`, with
`kind` an open set of strings. Queries reuse it rather than inventing an error type.

`path` becomes a `/`-separated address, most general first:

| path | means |
|---|---|
| `lots` | the query named `lots` |
| `lots/where/and/1/value` | a node inside that query |
| `lots/8a1e…` | a record returned by `lots` |
| `lots/8a1e…/qty` | a field of that record |

New `kind`s for queries: `unknown_collection`, `unknown_field`, `type_mismatch`,
`invalid_query`, `forbidden`, `cursor_mismatch`, `limit_exceeded`.

- **Query diagnostics** (errors) are all collected before execution and returned in that
  query's result. The query does not run.
- **Record diagnostics** (warnings) are read drift, as `/data/.../list` reports today. They go
  in the batch-level `diagnostics`, and never fail the query.

Writes will produce the same shape with `severity: error` and a record/field `path`. Custom
record- and field-level validation, when it arrives, emits more `kind`s into it.

## Authorization

Each query is checked on its own, for the read verb on its resolved collection, exactly as
`CollectionScope::require_can_read_data` does today: members read; token-less `public` reads
when a permission set the pinned version's manifest assigns to `public` grants `read` on that
collection. A denied query is a `forbidden` error in its result; the rest of the batch runs.

Site-level failures — no site, bad headers, an expired token — fail the whole request as they
do now.

## Lake

The lake stays schemaless. `loco-apps` resolves names, checks types, and authorizes; the lake
receives a resolved query in its own terms:

```rust
pub struct LakeQuery {
    pub collection: String,        // collection_key: "brickos/inventory.lot"
    pub filter: Option<Filter>,    // field names are storage keys; system fields are an enum
    pub order: Vec<OrderKey>,      // always ends with id
    pub limit: usize,
    pub after: Option<Vec<Value>>, // decoded cursor
    pub fields: Option<Vec<String>>,
}

pub trait DataAdapter {
    // …existing methods…
    fn query(&self, dataset_id: &str, queries: &[LakeQuery]) -> Result<Vec<Page>, Error>;
}
```

One call per batch, so the adapter owns the snapshot. `Page` is `records` plus the last order-key
values. The cursor encoding lives in `loco-apps`, not the lake.

Memory evaluates the filter in process. Sqlite compiles it to `json_extract(fields, '$.name')`
comparisons with bound parameters. Indexes on hot fields are a later concern; a full scan of one
`(dataset_id, collection)` is fine at today's sizes.

## Relationship to other issues

- **#15** (filter, sort, limit on `DataAdapter::list`) — this design replaces its public shape.
  #15 becomes the first implementation step: `LakeQuery` and `DataAdapter::query`.
- **#28** (qualified names) — Decision 4 implements the rule for queries. The rest of #28
  (`/schema`, existing `/data` routes, permission-set references) stays in #28.
- **#18** (`Array` / `Object` on `Value`) — list ops in `where` wait on it.
- **#47** (named actions) — a saved query could be a kind of named action. Not designed here.
- **#16** (sqlite `get` errors mapped to not-found) — the sqlite query path must not repeat it.

## Open questions

1. **Fields on a dependency's collection.** Under the rule as written, querying
   `acme/crm.contact` by `email` needs `acme/crm.email`, because bare `email` means the running
   project's field. That is strict and verbose. The alternative is that a bare field resolves in
   the *collection's* project. The answer also depends on whether a project can add fields to a
   dependency's collection at all, and how those fields would be keyed in a record's `fields`
   map, which is keyed by bare name today.
2. **Mixing `GET /data/{collection}/list?…` parameters with this.** Should `list` grow a
   query-string subset (`?limit=&cursor=`), or should everything but "all records" go through
   `POST /data/query`?
3. **Studio.** The collection table becomes the first real caller. Does it adopt `/data/query`
   in the same change as the endpoint, or after?

## Phasing

1. `LakeQuery`, `Filter`, `DataAdapter::query` for memory and sqlite, with unit tests on filter
   evaluation. (#15)
2. `POST /data/query`: parse, resolve names, type-check, authorize, execute, cursors,
   diagnostics. Hurl suite.
3. Studio and the BrickOS example move their client-side filtering onto it.
4. Mutations design document.
