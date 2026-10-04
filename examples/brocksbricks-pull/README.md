# Brock's Bricks pull

A phone-first pulling app for `brocksbricks/orders` ([`docs/brocksbricks.md`](../../docs/brocksbricks.md)): read the day's pending BrickLink orders once, plan them into a scale route for big lots and a hand route for small ones, each in bin walk order, and tick lines off while picking.

It is a hosted Loco frontend: a Vite build PUT as the bundle of `brocksbricks/orders@0.0.1-dev`, served by the `dev` site ([`docs/hosting.md`](../../docs/hosting.md)). It holds no secrets. Pickers sign in as Loco users with editor or developer access to the project.

## What it does

- **New pull run** reads `loco/bricklink.store:orders` with `status in [PENDING]`, then `loco/bricklink.store:order_items` with `order_id in […]`, once, from a button. Orders already allocated in an open run are skipped, and their lines are not read. The page says how many were skipped.
- **Plan** (`src/plan.js`, pure): lines are grouped by item, type, color, condition, and location, and summed across orders. A remark is located by a lookup, not a parser: trimmed and case-folded, it must equal a `location.code`. A located group at or above `settings.scale_threshold` is `scale`, below is `hand`. No match, or no remark, is `unlocated`, grouped by its remark and kept. A line whose quantity is missing or not a positive integer is never counted as 0: its whole order is held back, listed on the page, and read again by the next plan.
- **Create run** writes one `pull_run`, one `pick_line` per group, and one `pick_allocation` per order line.
- **Scale / Hand / Unlocated** list a run's lines in walk order, each with its per-order split, one chip per order (a lot in both batches of one order is summed). Tapping a line sets `picked_qty` to the full quantity, or clears it. **Count** records a short pick.
- **Mark done** closes the run, which releases its orders to the next plan.

## Setup

Follow [`docs/brocksbricks.md`](../../docs/brocksbricks.md), "Running it locally": its own server on `:3200`, the `brocksbricks` org, and the store pointed at the BrickLink mock on `:3100`. Then, as a developer on the project, add the two things a plan needs, through Studio or `/data`:

- one `settings` record with `scale_threshold` (with more than one, the oldest is used and the page warns);
- a `location` record per bin, `code` written exactly as it appears in lot remarks, and `walk_order`.

A run plans without locations, but every line is then unlocated.

From the repo root, once (the example is an npm workspace and imports `loco-client` from it):

```bash
npm install
```

## Deploy and open it

```bash
LOCO_USER=alice LOCO_PASSWORD=password npm run deploy
```

Builds `dist/`, zips it, and `PUT`s it to `/schema/brocksbricks/orders/0.0.1-dev/bundle` on `http://localhost:3200`. `LOCO_ORIGIN` points it at another server. Then open <http://dev.orders.brocksbricks.localhost:3200/>.

## Develop and test

```bash
npm run dev    # http://localhost:5178, proxied to :3200 (LOCO_ORIGIN overrides)
npm test       # node --test: the plan builder on fixture orders and lines
```

## Costs and limits

- **Reads.** A plan is 1 BrickLink call for the orders plus 1 per order not already in an open run: 51 for 50 orders. Nothing else in the app calls BrickLink. The lines come back 500 per page, and the source pages over a full upstream read with no cache, so a day with more than 500 order lines reads every order again for each further page (#141).
- **Writes.** `/data` has no batch insert, so a run costs 1 + lines + order lines requests: 560 for the mock's 50 orders (197 lines, 362 order lines). The app keeps 6 in flight. Batch writes are #142.
- **No transaction.** A run that fails part way is left partial and open. The error says how much was written. Marking the run done releases its orders, and the next plan reads them again. This also waits on #142.
- Fields on the BrickLink collections are `loco/bricklink`'s, so a query names them qualified (`loco/bricklink.order_id`). Records come back with bare names.
- A short pick is recorded on the line, not split across its orders.
