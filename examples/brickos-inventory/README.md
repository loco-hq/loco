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

Then, from the repo root (the example is an npm workspace, and imports `loco-client` from it):

```bash
npm install
```

## Deploy and open it

```bash
LOCO_USER=alice LOCO_PASSWORD=password npm run deploy
```

Builds `dist/`, zips it, and `PUT`s it to `/schema/brickos/inventory/0.0.1-dev/bundle`. Then open <http://dev.inventory.brickos.localhost:3000/>. Chrome and Firefox resolve `*.localhost` to loopback; Safari may not.

The bundle lands in `loco-apps/schemas/instances/brickos/inventory/versions/0.0.1-dev/bundle/`. `schemas/instances/` is the gitignored live store, so a deploy never shows up in `git status`.

## Develop

```bash
npm run dev   # http://localhost:5177, proxied to :3000
```

On `localhost:5177` there is no site host, so `src/data.js` creates the client with `projectId: 'brickos/inventory'` and `siteId: 'dev'` (dev builds only), and it sends them as headers. Hosted, the server infers both from the host name.

All API calls go through [`loco-client`](../../loco-client/src/index.js): `src/data.js` wraps it in TanStack Query hooks, and `App.jsx` reads the session from it with `useSyncExternalStore`.

## Limits

- The batch list still downloads every lot to count lots and pieces per batch; a batch page reads only its own lots through `POST /data/query`.
- Deleting a batch deletes its lots one request at a time; there is no cascade on `/data`.
- Adding a lot whose key (`item_no`, `color_code`, `condition`) the batch already holds adds to that lot. Editing a lot into a duplicate key is not prevented.
