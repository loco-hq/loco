# Brock's Bricks orders

`brocksbricks/orders` is the store project for Brock's Bricks' order pulling (issue [#105](https://github.com/loco-hq/loco/issues/105)). It is committed under `loco-apps/schemas/seed/brocksbricks/orders/` and seeded into the live store on first boot. It is schema only: `0.0.1-dev`, a `dev` dataset, and a `dev` site. The pulling app that uses it is [#106](https://github.com/loco-hq/loco/issues/106).

The manifest depends on `loco/bricklink@1.0.0`, so a store reads its BrickLink orders live as `loco/bricklink.store:orders` and `loco/bricklink.store:order_items` ([`integrations.md`](integrations.md)). That dependency is on disk only: `PUT /schema/.../manifest` refuses `loco/bricklink` for a writer who cannot read it, until projects can be marked installable ([#137](https://github.com/loco-hq/loco/issues/137)).

Pick state is this project's own lake collections. Fields on a collection belong to its owner, so pick progress cannot be a field on `order_items`.

## Collections

There is no date type. A timestamp is an ISO 8601 string in UTC (`2026-10-05T08:00:00Z`), which sorts as text.

| Collection | Fields | Notes |
|---|---|---|
| `location` | `code`\*, `zone`, `walk_order`\* (integer) | `code` is written as it appears in a lot's remarks. `walk_order` is the sort key of a snake path through the aisles. |
| `settings` | `scale_threshold`\* (integer) | One record. A pick line whose total is at or above the threshold goes to the scale station. |
| `pull_run` | `label`\*, `created_at`\* (ISO string), `status`\* (`open`, `done`) | One snapshot of pending orders. |
| `pick_line` | `pull_run_id`\*, `station`\* (`scale`, `hand`, `unlocated`), `walk_order` (integer), `location_code`, `item_no`\*, `item_type`\*, `color_id`\* (integer), `color_name`, `condition`\* (`N`, `U`), `qty`\* (integer), `picked_qty` (integer), `remarks` | One lot to pull: the same item, color, condition, and location summed across the run's orders. |
| `pick_allocation` | `pull_run_id`\*, `pick_line_id`\*, `bl_order_id`\*, `order_item_id`\*, `inventory_id`, `qty`\* (integer) | One order's share of a pick line. Scale lines are counted in bulk, then split into these. |

\* required.

- `item_no`, `item_type`, `color_id`, `color_name`, `condition`, and `remarks` are copied from `order_items` as BrickLink spells them. `condition` is `N` or `U`. `item_type` (`PART`, `SET`, `MINIFIG`, …) has no options, so a type the store has not sold before is not refused.
- An `unlocated` line has a remark that matches no `location.code`. It has no `location_code` and no `walk_order`, and it keeps its `remarks` so the picker can see what was written.
- `picked_qty` is empty until the line is picked.
- `order_item_id` is the `order_items` record id (`{order_id}-{batch}-{inventory_id}`), so an allocation leads back to its BrickLink line. `inventory_id` is optional because a BrickLink line can have none.
- `pull_run_id` is on the allocation too, so one query (`pull_run_id eq …`) reads a run's split without listing its lines first.
- There is no public permission set. Pickers log in.

## Running it locally

The `brocksbricks` org account is not created at boot, like `brickos` ([`brickos.md`](brickos.md)). The project loads anyway and logs one warning. Whoever creates the org owns it, and an org owner is a developer on every project under it.

1. Log in. Seeded `alice` has password `password`.

   ```bash
   TOKEN=$(curl -s localhost:3000/auth/login -H 'Content-Type: application/json' \
     -d '{"username":"alice","password":"password"}' | jq -r .data.token)
   ```

2. Create the org.

   ```bash
   curl -s localhost:3000/config/org -H "Authorization: Bearer $TOKEN" \
     -H 'Content-Type: application/json' -d '{"handle":"brocksbricks"}'
   ```

3. Point the store at an API and set its credentials on the `dev` dataset. Set `base_url` first, so that nothing reaches the real store API by accident. Until the store has real credentials, point it at the local mock once [#135](https://github.com/loco-hq/loco/issues/135) lands; the mock accepts any credentials, so any four values do. Secret writes need `LOCO_SECRET_KEY` on the server (`openssl rand -base64 32`).

   ```bash
   V=localhost:3000/config/variable/brocksbricks/orders/dev
   S=localhost:3000/config/secret/brocksbricks/orders/dev
   curl -s -X PUT "$V/loco%2Fbricklink.store:base_url" -H "Authorization: Bearer $TOKEN" \
     -H 'Content-Type: application/json' -d '{"value":"<the mock URL>"}'
   for name in consumer_key consumer_secret token_value token_secret; do
     curl -s -X PUT "$S/loco%2Fbricklink.store:$name" -H "Authorization: Bearer $TOKEN" \
       -H 'Content-Type: application/json' -d '{"value":"…"}'
   done
   ```

   `GET $S/list` shows which secrets are set. It never shows their values.

4. Read orders through the `dev` site:

   ```bash
   curl -s localhost:3000/data/loco%2Fbricklink.store:orders/list \
     -H "Authorization: Bearer $TOKEN" -H 'X-Project-Id: brocksbricks/orders' -H 'X-Site-Id: dev'
   ```

Studio shows the project for `alice` once the org exists.
