# Query

Supersedes the query shape sketched in #15.

**Status.** Phases 1 and 2 are implemented: `LakeQuery` and `DataAdapter::query` in
`loco-lake` (#15), and `POST /data/query` in `loco-apps` (#68, `src/query.rs`). The BrickOS
batch page reads its lots through it. Not implemented: Studio as a caller, list operators
(#18), the query-string question for `GET …/list`, and the mutations document. Choices the
implementation made where this design was silent are folded into the sections below, with the
rest under [Implementation notes](#implementation-notes).

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
      "order": [{ "field": "item_no", "collation": "natural" }, { "field": "color_code" }],
      "limit": 100
    },
    "colors": { "collection": "color", "fields": ["code", "label"], "limit": 500 }
  }
}
```

This is the BrickOS batch page in one request. Before this API it listed every lot and
filtered by `batch_id` in the browser.

- **Named.** Keys are client-chosen names (`[a-z_][a-z0-9_]*`). Results come back under the
  same keys. Names, not positions, so a diagnostic can say which query it's about. A key
  that breaks the pattern makes the body not a batch (400).
- **Independent.** A query that fails validation or authorization gets an error result. The
  others still run. The HTTP status is 200 when the batch itself was well-formed; it is 4xx only
  when the body can't be parsed as a batch at all: not JSON, not `{"queries": {…}}`, a bad
  query name, or more than 20 queries.
- **One snapshot.** Every query in a batch reads the same state: one read transaction in sqlite,
  one read lock in memory.
- **Bounded.** A maximum number of queries per batch (20) and a maximum `limit` per query
  (500; default 50). `limit` above 500 is `limit_exceeded`; 0 or a non-integer is
  `invalid_query`.
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

This is the rule in `CLAUDE.md` ("Name resolution", #28), which every `VersionSchema` lookup
follows.

- `"collection": "lot"` → `brickos/inventory.lot`, when the running version is a
  `brickos/inventory` version.
- `"collection": "acme/crm.contact"` → `contact` from dependency `acme/crm`. Must be a **direct**
  dependency in the manifest, or it's `unknown_collection`.
- A qualified name for the running project itself (`brickos/inventory.lot`) is accepted and is
  the same as the bare name.

Qualified collection names are already the lake's key: `collection_key` (`http/paths.rs`) stores
records under `{owner_project}.{name}`. Resolution produces that key directly.

Fields follow the same rule: `"field": "qty"` is the running project's `qty` field;
`"field": "acme/crm.email"` is `acme/crm`'s. A collection's fields are its owner's, so on a
dependency's collection a bare field never resolves: filtering `acme/crm.contacts` by `email`
is `unknown_field`; write `acme/crm.email`. Nor does a field some other project declares under
the same collection name. This is the conservative answer to
[Open question 1](#open-questions); loosening it later is not breaking. A field name resolves to
its bare name as the key in the record's `fields` map.

Resolution uses `VersionSchema::collection_in` / `field_in`, which look only in the named
project and only when it is self or a direct dependency.

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
| `lt` `lte` `gt` `gte` | number or string | ordered below / above (strings compare as bytes, whatever `order`'s collation; timestamps are RFC 3339 so this orders them). `null` or a boolean is `invalid_query`. |
| `in` | non-empty list of scalars | equal to one of them (each is type-checked; `null` is allowed) |
| `exists` | `true` / `false` | present and not null / absent or null |

- **Missing is null.** The lake is schemaless, so a record can lack a field. For every op
  except `exists`, a missing field behaves as `null`.
- **Null only matches null.** `eq null` matches null or missing. Ordering ops never match null.
- **`ne` is exactly `not eq`.** So `qty ne 3` matches null, missing, and a drifted `"3"`. "A
  drifted value doesn't match" below applies only to `eq`, `in`, and the ordering ops. Sqlite's
  kind expression is never SQL NULL, so its `NOT` agrees with the memory evaluator.
- **Integer and float are one kind.** `3` equals `3.0` and they order together. The comparison
  is exact, never rounded through f64: `2^53 + 1` and `2^53` as a float are different, on both
  adapters. A `float` field accepts an integer value.
- **Values are type-checked against the field**, up front: `qty gt "3"` is a `type_mismatch`
  error on the query, not an empty result. A record whose stored value has drifted to another
  type doesn't match (and gets a read warning if returned by some other condition).
- Depth and size are bounded: depth 8 (a lone comparison is depth 1), 100 comparisons. Over
  either is `limit_exceeded`.
- `{"and": []}` matches every record and `{"or": []}` none. A combinator must be the only key
  in its object.
- A field of type `list` cannot be filtered or ordered by (`invalid_query`) until #18. `/schema` no longer creates one (#13); only field YAML written before that has it.
- No text search, no regex, no list ops in v1. List ops arrive with #18.

### `order`

A list of `{ "field": name, "dir": "asc" | "desc", "collation": "binary" | "natural" }`, `dir`
defaulting to `asc` and `collation` to `binary`. Every order is
total, which is what makes a cursor possible. The lake makes it so: it appends `id ASC` unless
the last key is already `$id`, in either direction, and never appends a second one. An empty
`order` becomes `[id ASC]` alone.

With no `order`, the API default is `[$created_at, $id]`. `loco-apps` applies that before
calling `DataAdapter::query`; the lake has no default of its own.

Values sort by kind first, then value: null < boolean < number < string, with `false < true`.
So nulls sort first ascending and last descending, and a value that drifted to another type sorts
with its kind. A total order across kinds is what lets a cursor resume after any value.

#### Collation

`collation` decides how two strings compare under that key. It changes nothing else: kinds
still sort as above, numbers still compare as numbers, and `where` always compares bytes.

- **`binary`** (default): bytewise. `10247` < `3001` < `3001pr0001`.
- **`natural`**: digit runs compare by numeric value. `3001` < `3001pr0001` < `10247`, and
  `a2` < `a10`. For part numbers, SKUs, and version-like codes, so an app need not re-sort in
  the browser and break paging.

`natural`, exactly (`loco_lake::natural_cmp`):

1. Walk both strings from the start. Where both are at an ASCII digit, take the whole run of
   ASCII digits on each side and compare the runs by value: strip leading zeros, then the
   longer run is larger, then compare bytes. Runs are never parsed into an integer, so any
   length works.
2. Everywhere else compare one byte at a time. A digit run therefore ranks against a
   non-digit byte as its first digit would: `a-` < `a1` < `a:` < `aa`. Non-ASCII compares
   as its UTF-8 bytes, as in `binary`.
3. A string that runs out first is smaller: `3001` < `3001pr0001`.
4. **Leading zeros.** Strings equal under 1–3 differ only in leading zeros. Byte order
   breaks that tie, so `007` < `07` < `7` < `008`. Two strings are equal only if they are
   identical, which keeps the order total and deterministic without leaning on `$id`.

`natural` on an `integer`, `float`, or `boolean` field would change nothing, so it is
`invalid_query`. It is allowed on system fields, which are strings.

### `limit` and `cursor`

The response carries `cursor`: `null` when there is nothing more, otherwise an opaque string.
Send it back as `"cursor"` on the same query to get the next page.

- A cursor encodes the order-key values of the last record returned (keyset pagination), not an
  offset. Inserts and deletes between pages don't skip or repeat records.
- It also encodes a hash of the query's `collection`, `where`, and `order`, collation
  included. A cursor sent with a different query is a `cursor_mismatch` error, so a cursor
  from a `natural` order can't resume a `binary` one. `limit` and `fields` may change between
  pages.
- It is opaque and unsigned. Tampering with one can only produce a different page of a query
  the caller is already allowed to run.
- Encoding: base64url, no padding, of `{"h": hash, "k": [values]}`. The hash is over the
  *resolved* query, so `lot` and `brickos/inventory.lot`, an omitted `dir` and `"asc"`, or an
  omitted `collation` and `"binary"`, are the same query. A `binary` key hashes as it did
  before collation existed, so cursors issued before it still resume. A cursor that doesn't decode, or has the wrong number of values, is
  `invalid_query`.
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
  query's result. The query does not run. Two exceptions stop early: an unresolvable
  `collection` (nothing else can be checked without it) and `forbidden` (a caller who may not
  read a collection is not told about its fields). A bad cursor is reported alone, since it is
  only checked once the rest of the query is valid.
- An unknown key in a query, a condition, or an order key is `invalid_query`, pointed at that
  key.
- **Record diagnostics** (warnings) are read drift, as `/data/.../list` reports today, checked
  the same way (`validate_records`, against the collection's fields). They go in the
  batch-level `diagnostics`, and never fail the query. With `fields`, only the projected fields
  are checked.

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
    pub order: Vec<OrderKey>,      // field, dir, collation; always ends with id
    pub limit: usize,
    pub after: Option<Vec<Value>>, // decoded cursor
    pub fields: Option<Vec<String>>,
}

pub trait DataAdapter {
    // …existing methods…
    fn query(&self, dataset_id: &str, queries: &[LakeQuery]) -> Result<Vec<Page>, Error>;
}
```

One call per batch, so the adapter owns the snapshot: memory holds one read lock for the call,
sqlite runs every query in one deferred transaction. `Page` is `records` plus `next`: the last
record's effective order-key values, present only when more records follow (the adapter fetches
`limit + 1` to know). `loco-apps` returns `cursor: null` exactly when `next` is `None`. It never
guesses from `records.len() == limit`, which would hand out a cursor to an empty page. The cursor
encoding lives in `loco-apps`, not the lake.

System fields are `FieldRef::System(SystemField::…)`, an enum, never keys in `fields`.

The lake checks structure and treats a failure as a caller bug. `limit` 0, an empty `in`, an
ordering op against null or a boolean, a field name containing NUL, or an `after` of the wrong
length fail the whole call with `Error::InvalidQuery`, because `query` returns one `Result` for
the batch. So `loco-apps` checks each of these per query first and reports it as that query's
`invalid_query`. None of them reach the lake from `/data/query`, and one bad query never turns
the request into a 400.

Field names reach sqlite as a bound, quoted JSON path (`$."name"`), with `\` written `\\` and
`"` written `\"`, so `.`, `[`, `\`, `"`, and non-ASCII names all work. NUL is the exception:
sqlite's path lookup stops there even when escaped, so both adapters reject a field name
containing NUL with `Error::InvalidQuery`, and `loco-apps` reports it as the query's
`invalid_query` first. Schema field names are slugs, so none has one.

Memory evaluates the filter in process. Sqlite compiles it to `json_extract(fields, '$.name')`
comparisons with bound parameters, each guarded by a `json_type` kind check (without it,
`json_extract` returns `true` as `1` and `flag eq 1` would match). Both adapters apply the
`fields` projection in Rust, in `finish_page`, after sorting, `after`, and `next`, so they return
the same rows.

`Collation::Natural` is one Rust function, `natural_cmp`. Memory calls it; sqlite registers
it on the connection as the collation `loco_natural` (not `natural`, a keyword) and writes
`json_extract(…) COLLATE loco_natural` in both the ORDER BY and the keyset `after`
predicate, so a page boundary resumes under the order that produced it. The shared adapter
tests walk every page size in both directions, over ties, nulls, missing and drifted values,
leading zeros, and digit runs longer than any integer. Nothing requires the projection to happen in SQL. Indexes on hot fields are a later concern; a full scan of one
`(dataset_id, collection)` is fine at today's sizes.

## Relationship to other issues

- **#15** (filter, sort, limit on `DataAdapter::list`) — this design replaces its public shape.
  #15 becomes the first implementation step: `LakeQuery` and `DataAdapter::query`.
- **#28** (qualified names) — Decision 4 is the rule for queries. #28 applied it to `/schema`,
  the `/data/{collection}` routes, and permission-set references too.
- **#18** (`Array` / `Object` on `Value`) — list ops in `where` wait on it.
- **#47** (named actions) — a saved query could be a kind of named action. Not designed here.
- **#16** (sqlite `get` errors mapped to not-found) — the sqlite query path must not repeat it.

## Open questions

1. **Fields on a dependency's collection.** *Answered conservatively for v1 (see [Names](#4-names)):
   a collection's fields are its owner's, and bare means the running project.* So querying
   `acme/crm.contact` by `email` needs `acme/crm.email`. That is strict and verbose. The alternative is that a bare field resolves in
   the *collection's* project. The answer also depends on whether a project can add fields to a
   dependency's collection at all, and how those fields would be keyed in a record's `fields`
   map, which is keyed by bare name today.
2. **Mixing `GET /data/{collection}/list?…` parameters with this.** Should `list` grow a
   query-string subset (`?limit=&cursor=`), or should everything but "all records" go through
   `POST /data/query`?
3. **Studio.** The collection table becomes the first real caller. Does it adopt `/data/query`
   in the same change as the endpoint, or after?

## Phasing

1. Done (#15). `LakeQuery`, `Filter`, `DataAdapter::query` for memory and sqlite, with tests on
   filter evaluation against both.
2. Done (#68). `POST /data/query`: parse, resolve names, type-check, authorize, execute,
   cursors, diagnostics. Hurl suite `data_query`.
3. The BrickOS batch page's lots and its batch delete use it (#68). Studio has not moved.
4. Mutations design document.

## Implementation notes

Choices the code made where the design above was silent, kept here so the next change starts
from them. Most are folded into the sections above; these don't have a better home.

- **A dependency's records** are written through `/data/{collection}` with the qualified name
  percent-encoded (`/data/acme%2Fcrm.contacts/add`), under the same lake key a query reads.
- **Authorization** is the same rule as `CollectionScope::require_can_read_data`, now on
  `SiteScope::may_read_collection` so a query can ask it per collection.
- **BrickOS ordering.** The batch page orders lots by `item_no` with `collation: natural`
  (#72). It used to re-sort each page in the browser with a numeric-aware collator.
