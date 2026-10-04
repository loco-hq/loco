# BrickLink fixture bodies

Response bodies of the BrickLink store API, in its `{meta, data}` envelope, for the
calls `loco/bricklink` makes. The `bricklink` Hurl suite serves them from an
in-process server (`start_bricklink_upstream` in `tests/hurl_runner.rs`), and the
source's unit tests parse them. The local mock (`examples/bricklink-mock.rs`,
`docs/brickos.md`) builds every order and line it serves from `orders/29471234.json`
and `orders/29471234/items.json`, and its envelopes from `errors/`. Its tests check
that its bodies have exactly these keys, so renaming or dropping a key here breaks
`cargo test` until the mock follows.

A request path maps to a file by appending `.json` to it, under this directory:

| Request | File |
|---|---|
| `GET /orders?direction=in[&status=A,B]` | `orders.json`, filtered by `status` when given |
| `GET /orders/{id}` | `orders/{id}.json` |
| `GET /orders/{id}/items` | `orders/{id}/items.json` |
| `/orders/…` without a file | `errors/not_found.json` (`meta.code` 404, HTTP 200) |
| a path outside `/orders` | bare HTTP 404, `text/plain`, no envelope (a wrong `base_url`) |

`errors/bad_oauth.json` is what a request with an unregistered consumer key gets. It
is HTTP 200, as BrickLink answers, with the error in `meta.code`. The fixture server
replaces `{oauth_consumer_key}` with the key the request sent, so the suite can
check that the source never passes a credential back to the caller.

`29471200` is a filed order: `GET /orders/29471200` answers it, and the source
treats it as not found because `orders` holds orders that are not filed.

`orders/29471234/items.json` has two batches. Lot `358001234` is in both, and the last line of batch 2 has `inventory_id: null`. The source must return all five lines with distinct ids.

Ids, buyers, and remarks are made up. Quantities and color ids are integers, as
BrickLink returns them.
