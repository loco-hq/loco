# BrickOS inventory

Target model. The first schema slice, issue [#48](https://github.com/loco-hq/loco/issues/48) — stored batches, lots, and the catalog — is committed under `loco-apps/schemas/seed/brickos/inventory/` and seeded into the live store on first boot. Nothing past it is implemented. Unions, differences, views, and selling are the model those records have to grow into. They wait on query ([#15](https://github.com/loco-hq/loco/issues/15)) and a named action runner ([#47](https://github.com/loco-hq/loco/issues/47)).

BrickOS is one Loco project, `brickos/inventory`, used for a LEGO reseller: parts, sets, and minifigs listed on BrickLink, Amazon, and smaller marketplaces. A batch is the only inventory shape. A set's contents, a minifig's parts, a purchase intake, a commission pile, and an order are all batches. Another store later is another project that depends on this schema and keeps its own dataset. Store-specific fields land on that store's version. This document is the inventory model, not that packaging work.

Agents use the ordinary record API plus, later, named actions. Part-out math, union, difference, and allocating a sale are actions. The agent supplies the arguments and then reads and updates records.

## Vocabulary

| Term | What it is |
|---|---|
| **Item** | A catalog entry: a part, set, or minifig, identified by `item_type` + `item_no`. No color. |
| **Color** | A BrickLink color id and a label. |
| **Lot key** | `(item_no, color_code, condition)`. The identity union and difference match on. Location, price, and remarks are not part of it. |
| **Lot** | One quantity of a lot key. Always belongs to one batch. |
| **Batch** | A named list of lots. `kind` is one of the field's `options` (`inventory`, `set`, `minifig`, `order`, `commission`); a new kind is a schema edit, and `/data` rejects anything else. `mode` is `stored` or `view`. |
| **Stored batch** | Owns lot records. Editing a lot edits the batch. |
| **View** | A batch that stores an operation and its inputs, and computes lots when read. It has no lot records of its own. |
| **Materialize** | Evaluate a view (or any operation) and write the result into a new stored batch. The new batch does not keep the inputs. |
| **Allocation** | A record that says a quantity on an order came from a particular stored source batch. Settlement reads this. On-hand counts do not. |

`item_no` and `color_code` are copied onto the lot. A lot can name a part that has no catalog row yet.

## Catalog and the set-as-batch link

`item_type`, `color`, and `item` are reference rows. A set or minifig in the catalog is an `item` plus a stored batch of its parts: the batch's `item_no` is that item, and its `kind` is `set` or `minifig`. The Death Star's bill of materials is that batch, with quantities for one set. Parting out 35 is an action that reads that batch, multiplies every quantity by 35, and merges the result into a destination batch. The multiply step does not live in the agent.

## Stored batches

A stored batch is the #48 shape. Lots are their own records (`batch_id`, lot key, `qty`) because a record value cannot hold a list.

On-hand inventory the seller owns is a stored batch. A commission pile is a different stored batch, so the two inventories stay separable even when they are listed together. An order is a stored batch of the lines that were sold. A bin-level detail, when it exists, is extra fields on the stored lot. The for-sale view sums those lots away by lot key.

## Views

A view batch has `mode: view`, an `op` of `union` or `difference`, and input rows (a `batch_input` collection: the view id, the source batch id, and a role). Inputs may themselves be views. Evaluating a view walks those inputs, detects a cycle, and returns computed lots. Nothing is written.

Input rows are their own collection so the definition does not need a list field.

**Union.** Sum quantity by lot key across every input with role `member`. Order does not change the result. Own stock of 10 black 2x4 plates (part `3001`, color `11`, new) unioned with a commission batch of 4 is one lot of 14.

**Difference.** One input has role `base`. Every input with role `subtract` is unioned together and removed from the base, matching on lot key. Quantity clamps at 0. A key present only on the subtract side does not appear in the result. The same plates, base 14 minus an open order of 6, is 8.

Clamping is the inventory meaning of difference. A shortage (subtract side larger than the base) is a separate report on the same evaluation, not a negative lot.

## Listing several inventories as one

The quantity listed for sale is a view, not a copy of the piles:

```
for_sale  = union(own, commission)
available = difference(for_sale, open_orders)
```

`open_orders` is itself a union of order batches that have been sold and not yet pulled. Adding a commission deal is adding one input to `for_sale`. The own-stock batch is untouched, and the commission batch is still there when the deal ends and the input is removed.

A marketplace upload that needs a frozen file materializes `available` into a new stored batch and uploads that. The view keeps moving as orders arrive. The snapshot does not.

## Selling

Selling writes an order batch and allocation records. It does not change on-hand lots.

The arguments are the order lines (a stored batch), the pool being sold from (usually the `available` view, or the stored batches underneath `for_sale`), and a policy. The policy names **stored** source batches. A view is not a source of attribution.

**Precedence.** Walk the source batches in order. For each lot key on the order, take as much as that source still has, then the next source. Commission first, then own: an order for 6 against commission of 4 and own of 10 attributes 4 to commission and 2 to own.

**Split.** Each source has a weight. Ideal quantity is the order quantity times that weight over the sum of weights. A source short of its ideal contributes what it has. The unmet quantity is offered to the other sources in input order, and those sources still stop at their on-hand. A 50/50 split of 10, with commission holding 4 and own holding 10, ideals 5 and 5, attributes 4 to commission and 6 to own.

Allocations are records: order batch, source batch, lot key, quantity. They are how commission gets paid. They also tell a pull which stored batch to walk. The bin is whichever lot inside that stored batch has the key. The view does not know bins.

Availability uses the order batch, not the allocation. While the order is open, `available` subtracts it, so the same plates cannot be sold twice. A pull decrements the source lot and takes the order out of `open_orders` by the same quantity (delete the line, or drop the order from the union once every line is pulled). On-hand and the reservation fall together, and `available` stays put.

## Materialize and merge

**Materialize** evaluates a view, or a one-off union or difference, and inserts a new stored batch with those lots. Use it for a snapshot: a set inventory saved on a date, a file for a marketplace, a pile about to be boxed. Later edits to the inputs do not change it.

**Merge** adds one stored batch into another stored batch, matching on lot key. Quantities add. A key the destination does not have becomes a new lot. Inputs other than the destination are unchanged. Parting out 35 Death Stars is merge: the multiplied set batch is merged into intake or into on-hand. Union would only have described the combination. Merge is what changes the shelf.

Where a newly created lot sits (which bin, which "needs a home" batch) is an argument of that merge, added when locations exist. The operation does not need locations to sum quantities.

## What issue #48 actually adds

Stored mode only. Collections: `item_type`, `color`, `item`, `batch`, `lot`. A batch has `label`, `kind`, and `item_no`. A lot has `batch_id` and the lot key and `qty`. No `mode`, no `batch_input`, no allocation, no evaluation.

That is enough to record a set's parts, an order, and a single on-hand pile, and to do it through ordinary record CRUD.

## Later, on top of that schema

Each of these is an action behind the #47 runner, except the read of a view, which is a query once a batch knows its inputs.

- `mode` and `op` on `batch`, plus `batch_input`.
- Evaluate `union` and `difference`, including a view of views, with a cycle check.
- Materialize the evaluation into a new stored batch.
- Merge one stored batch into another.
- Sell: write the order batch and the allocations from a precedence or split policy, leaving on-hand lots as they are.
- Part out: multiply a set's stored batch by a count and merge.

Query ([#15](https://github.com/loco-hq/loco/issues/15)) is what makes "lots in this batch with this part" a list call instead of a full-collection download. The actions want the same filter internally.

## Developing against BrickLink without credentials

There is no BrickLink sandbox, and a store's API credentials need a registered seller account. `loco/bricklink` (`docs/integrations.md`) reads the store API at the `base_url` variable, set per dataset, so a dev dataset can point at a local mock while the real dataset points at BrickLink.

```bash
cargo run -p loco-apps --example bricklink-mock
# BrickLink mock on http://127.0.0.1:3100: 65 orders (50 PENDING, 3 filed), 616 lines, 2026-10-03 seed 135
```

Flags: `--port` (default `3100`), `--seed` (default `135`), and `--date` (default `2026-10-03`), as in `cargo run -p loco-apps --example bricklink-mock -- --seed 7`. The same seed and date serve the same day on every run.

It answers the three calls the source makes: `GET /orders?direction=in` (with `status`, comma-separated, and a leading `-` excludes), `GET /orders/{id}`, and `GET /orders/{id}/items`. It accepts any `Authorization: OAuth …` header and does not check the signature. The signer is unit-tested, and the `bricklink` Hurl suite checks signatures on the wire. Each request is logged to stderr.

The day has 50 `PENDING` orders, six `PAID`, four `PACKED`, two `SHIPPED`, and three filed `COMPLETED` orders. `GET /orders/{id}` answers a filed order; the list leaves it out, as BrickLink does. Lines are parts, sets, and minifigs drawn from one store inventory, so a lot and its bin recur across orders. Quantities run from 1 to several hundred, so both scale-weighed and hand-counted lines are there. A lot's `remarks` is its bin: `A-12-3` (aisle, shelf, bin) for a part, `MF-4` for a minifig, and `SET-2` for a set. About one lot in twenty has a malformed remark, such as `A12-3`, `C-1-2-3`, `see blue drawer`, or an empty one. A big order sometimes arrives in two batches, now and then with the same lot on a line in each. The source keeps both lines. A path outside `/orders` is a bare HTTP 404, so a `base_url` with a stray path fails loudly instead of returning no orders.

It is a Rust example rather than a script so it needs nothing new: axum, tokio, and chrono are already `loco-apps` dependencies. It builds every body from the fixture files the Hurl suite serves (`loco-apps/tests/fixtures/bricklink/`), with values replaced, and replacing a key the fixture lacks panics. CI compiles it under clippy and `cargo test` runs its tests, including one that its orders and lines have exactly the fixture's keys. The mock and the fixture cannot drift apart in shape.

### Pointing a dataset at it

The store's version must depend on `loco/bricklink@1.0.0`. `PUT /schema/.../manifest` refuses that dependency for a writer who cannot read `loco/bricklink`, until projects can be marked installable. So write the manifest in the live store and restart the server:

```yaml
# loco-apps/schemas/instances/{account}/{project}/versions/{version}/manifest.yaml
dependencies:
  - loco/bricklink@1.0.0
```

The server needs `LOCO_SECRET_KEY` (`openssl rand -base64 32`) to store secrets. Set `base_url` first, so no request reaches the real store API, which is the default. Then set the four secrets. They are required, so the source refuses to run without them, and any value works against the mock:

```bash
V=http://localhost:3000/config/variable/alice/brocks/dev
S=http://localhost:3000/config/secret/alice/brocks/dev
H=(-H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json')
curl -X PUT "${H[@]}" "$V/loco%2Fbricklink.store:base_url" -d '{"value":"http://127.0.0.1:3100"}'
for name in consumer_key consumer_secret token_value token_secret; do
  curl -X PUT "${H[@]}" "$S/loco%2Fbricklink.store:$name" -d '{"value":"mock"}'
done
```

Then read the generated orders through the site, and their lines with an `order_id` filter. `order_items` has no unfiltered list:

```bash
curl "${H[@]}" -H 'X-Project-Id: alice/brocks' -H 'X-Site-Id: dev' \
  http://localhost:3000/data/query -d '{"queries":{
    "pending":{"collection":"loco/bricklink.store:orders",
               "where":{"field":"loco/bricklink.status","op":"eq","value":"PENDING"},
               "limit":100},
    "lines":{"collection":"loco/bricklink.store:order_items",
             "where":{"field":"loco/bricklink.order_id","op":"in","value":["29480774","29480778"]}}}}'
```

To use the real store, set the four secrets to its credentials and delete `base_url` (`DELETE $V/loco%2Fbricklink.store:base_url`), so it falls back to the default. Use a different dataset for that, and keep the mock on `dev`. No code changes.

## Non-goals

Marketplace sync, price guides, and container capacity. BrickLink remains the listing system of record until a materialized batch is deliberately uploaded.

Decrementing on-hand at the moment of sale. The order batch is the reservation. The pull is what changes the shelf.

A workflow language for these operations. The operations are code handlers. A declarative form can become another handler later without changing what a batch is.
