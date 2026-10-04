# BrickLink fixture bodies

Response bodies of the BrickLink store API, in its `{meta, data}` envelope, for the
calls `loco/bricklink` makes. The `bricklink` Hurl suite serves them from an
in-process server (`start_bricklink_upstream` in `tests/hurl_runner.rs`), and the
source's unit tests parse them. A local mock (#135) should serve these same files so
the two cannot drift.

A request path maps to a file by appending `.json` to it, under this directory:

| Request | File |
|---|---|
| `GET /orders?direction=in[&status=A,B]` | `orders.json`, filtered by `status` when given |
| `GET /orders/{id}` | `orders/{id}.json` |
| `GET /orders/{id}/items` | `orders/{id}/items.json` |
| anything without a file | `errors/not_found.json` |

`errors/bad_oauth.json` is what a request with an unregistered consumer key gets. It
is HTTP 200, as BrickLink answers, with the error in `meta.code`. The fixture server
replaces `{oauth_consumer_key}` with the key the request sent, so the suite can
check that the source never passes a credential back to the caller.

`29471200` is a filed order: `GET /orders/29471200` answers it, and the source
treats it as not found because `orders` holds orders that are not filed.

Ids, buyers, and remarks are made up. Quantities and color ids are integers, as
BrickLink returns them.
