# BrickOS inventory

A batch editor for `brickos/inventory` ([`docs/brickos.md`](../../docs/brickos.md)): list batches, open one, and add, edit, or delete its lots. Catalog `color` and `item` rows, when they exist, show as labels under the raw codes.

It is a hosted Loco frontend: a Vite build PUT as the bundle of `brickos/inventory@0.0.1-dev`, served by the `dev` site ([`docs/hosting.md`](../../docs/hosting.md)). It holds no secrets; you sign in as a Loco user with editor or developer access to the project, because nothing in inventory is public.

## One-time setup

The `brickos` org is not seeded. With `cargo run -p loco-apps` running, create it as your user — the creator owns it, and an org owner is a developer on every project under it:

```bash
TOKEN=$(curl -s localhost:3000/auth/login -H 'Content-Type: application/json' \
  -d '{"username":"alice","password":"password"}' | jq -r .data.token)
curl -s localhost:3000/config/org -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"handle":"brickos"}'
```

```bash
cd examples/brickos-inventory
npm install
```

## Deploy and open it

```bash
LOCO_USER=alice LOCO_PASSWORD=password npm run deploy
```

Builds `dist/`, zips it, and `PUT`s it to `/schema/brickos/inventory/0.0.1-dev/bundle`. Then open <http://dev.inventory.brickos.localhost:3000/>. Chrome and Firefox resolve `*.localhost` to loopback; Safari may not.

The bundle lands in `loco-apps/schemas/instances/brickos/inventory/versions/0.0.1-dev/bundle/`, which is gitignored: the build is not source.

## Develop

```bash
npm run dev   # http://localhost:5177, proxied to :3000
```

On `localhost:5177` there is no site host, so the client sends `X-Project-Id: brickos/inventory` and `X-Site-Id: dev` itself (dev builds only). Hosted, the server infers both from the host name.

## Limits

- `/data` has no filter yet ([#15](https://github.com/loco-hq/loco/issues/15)): a batch page downloads every lot and filters by `batch_id` in the browser.
- Deleting a batch deletes its lots one request at a time; there is no cascade on `/data`.
- Adding a lot whose key (`item_no`, `color_code`, `condition`) the batch already holds adds to that lot. Editing a lot into a duplicate key is not prevented.
