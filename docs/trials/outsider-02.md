# Outsider trial 02

A fresh Claude Code session, with no Loco checkout and no `CLAUDE.md`, was asked to build the same inventory app as trial 01. It signed up, created a schema, wrote six records, and published a web UI. No hint was sent.

The discovery guide's worked example is a LEGO parts inventory (handle `lego_reseller`, project `inventory`, a public list-and-edit page published as `www`), so this task was close to the guide's own example; the result shows the guide can be followed end to end, not that an outsider can build an arbitrary app. Trial 03 should use a task that is not inventory-shaped and not in the guide: a store that depends on `loco/bricklink@1.0.0` and reads `store:orders` (that reaches #137), or a declared action, or a member-login page.

The transcript is [`outsider-02-transcript.md`](outsider-02-transcript.md). Passwords and the session token are redacted there. This file is the observer's account. The comparison with trial 01 is below.

## Setup

- Date: 2026-10-09. Server commit `b93ec547e10e05905d0ff7d6002166b280e26040` (this worktree, `issue-159-outsider-trial-02`, `origin/main`).
- Binary: `target/debug/loco-apps`, built in this worktree. Process id 78076. Bound `0.0.0.0:3300` only.
- `LOCO_ROOT` and `LOCO_DB_PATH` were under a new temp directory (`/private/var/folders/wg/7y8wv38j5x7bt2rn6hmz837r0000gn/T/loco-outsider-02-root.YEuyJ7`). `schemas/types` (14 files) and `schemas/seed` (84 files) were copied in before start. The process environment was otherwise empty of Loco variables: `LOCO_AUTH_AUTO_CREATE`, `LOCO_DEFAULT_SITE`, `LOCO_SECRET_KEY`, and `LOCO_ADAPTER` were unset. The adapter defaulted to SQLite.
- Boot log: seeded `brickos/inventory`, `brocksbricks/orders`, `loco/bricklink`, `loco/core`, `loco/demo`, and `loco/studio`. Then `LOCO_SECRET_KEY is not set; PUT /config/secret will return 503`, SQLite at the temp `loco.db`, and local auth under that root. The auth adapter's usual seed wrote `alice`, `bob`, and the `loco` org into that temp auth directory. The subject never logged in as any of them. Boot also warned that the seeded `brickos` and `brocksbricks` accounts do not exist.
- A request with no token, `GET /auth/me`, returned `401` `{"ok":false,"error":"missing auth: use Authorization: Bearer <token> header"}`.
- Ben's server on port 3000 was not used. Nothing in this trial bound that port. Port 3000 was not listening.
- Subject: Claude Code v2.1.295, Opus 5.5, started with `herdr agent start subject2 --kind claude --pane w1G:p2 -- --permission-mode auto` in an empty temp directory outside the repository (`…/T/loco-outsider-02-subject.Q71mVu`). The pane was a split of the observer's workspace. The flag was accepted. The status line read `auto mode on`.
- Before the session would take a prompt, the pane blocked on the workspace trust check (`Yes, I trust this folder`). The observer selected that line and confirmed with Enter. That was the only dialog. It records trust for this temp directory in the local Claude settings. No "always allow" choice was offered or taken. The trust entry was left in place. No settings file was edited.

## Task

Sent at 2026-10-09T16:05:51Z, the trial 01 task text verbatim (the origin was already `http://localhost:3300`):

> Build a small inventory app on this Loco server for a LEGO reseller: batches of parts with quantities. Give it a schema, seed a few records, and publish a web UI that lists and edits them, hosted by Loco.
>
> The server is at http://localhost:3300. You must sign up for your own account. You must not read the loco-hq/loco GitHub repository or any Loco source or docs — only what the server itself answers and general web knowledge.

No hint was sent. The subject finished the turn at 16:07:51Z (`durationMs` 120047, "Worked for 2m 0s") and stopped. The prompt line then showed an unsent suggestion, `open it in the browser and click through it`. It was not submitted. The working directory was still empty. The account, schema, records, and bundle are on the throwaway server. The HTML was written in the session scratchpad, outside the repository.

## Timeline

### 1. The origin 404 names the discovery document — 16:05:53Z

`GET /` and `GET /api` both returned `404`. Each body now includes `see`:

```
{"error":"no such endpoint: /","ok":false,"see":"/.well-known/loco.json"}
{"error":"no such endpoint: /api","ok":false,"see":"/.well-known/loco.json"}
```

The subject requested `GET /.well-known/loco.json` next, at 16:05:55Z. The body is about 43KB. Claude Code stored it as a tool-result file and showed a preview. The preview's first guide step says to follow the document in order and not to guess a path.

### 2. Auto mode refuses `cat`, and Read succeeds — issued 16:05:58Z, refused 16:06:14Z

`cat` of that tool-result file was issued at 16:05:58Z. The classifier answered at 16:06:14Z, so 16 of the 120 seconds went to the refusal:

```
Permission for this action was denied by the Claude Code auto mode classifier. Reason: [PII Data Handling].
```

The same file opened with the Read tool at 16:06:16Z. The subject then wrote "Clear guide. I'll sign up with my own handle and script the setup."

The document's `signup.handle` says a handle is 1–63 characters of `a-z`, `0-9`, and `_`, starting with a letter or `_`, and that `lego_reseller` is legal and `lego-reseller` is not. `site.note` says `http://localhost:3300/` stays JSON after publish, and gives the site URL `http://{site}.{project}.{account}.localhost:3300/`. It also says hosted files are public and not to put a bearer token in the page, and it names issue #145. A guide step says this server registers no action handlers and not to look for a CLI.

### 3. Signup and the project — 16:06:24Z

One shell command signed up, logged in, and created the project. The handle was `brickvault_` plus three random hex characters (`brickvault_b5ebd8`). The password was `openssl rand -hex 16`, written to the scratchpad and expanded as `$P`. The signup body came back:

```
{"ok":true,"data":{"id":"ce2becd4-e88f-418d-bc30-0246f0c300d3","username":"brickvault_b5ebd8","name":"Brick Vault","account_type":"person","created_at":"2026-10-09T16:06:24.528542+00:00","last_login_at":null}}
```

`curl -s` did not print the status line. The account file is `auth/accounts/brickvault_b5ebd8.json` under the temp root. `POST /config/project` with `name` `inventory` returned the project, and `GET /config/site/brickvault_b5ebd8/inventory/dev` returned site `dev` pinned to `0.0.1-dev` and dataset `dev`.

The subject did not send a hyphenated handle, so this run did not exercise the signup `400`. It also did not create an org, so the org-handle errors from #150 were not exercised.

### 4. Schema — 16:06:32Z

`POST` under `/schema/brickvault_b5ebd8/inventory/0.0.1-dev` created collection `batches`, nine fields, permission set `batches_public`, and then `PUT` the manifest. Every response was `{"ok":true,…}`.

| Field | Type | Required |
|---|---|---|
| `batch_code`, `part_number`, `part_name` | string | yes |
| `quantity` | integer | yes |
| `condition` | string, options `new` and `used` | yes |
| `color`, `location` | string | no |
| `unit_price` | float | no |
| `listed` | boolean | no |

`batches_public` grants `read`, `create`, and `update` on `batches`, and not `delete`. The manifest sets `public_permission_sets` to that name and `dependencies` to `[]`. Creating the fields also wrote the server's default auto-add fieldset; the subject did not `POST` a fieldset.

### 5. Six records — 16:06:40Z

`POST /data/batches/add` with `Authorization: Bearer $T`, `X-Project-Id: brickvault_b5ebd8/inventory`, and `X-Site-Id: dev` inserted six batches. Each line of output was `True` and a UUID. The rows are `B-2026-001` through `B-2026-006` (2×4 red bricks, 1×2 black plates, trans-clear round bricks, used minifig torsos, a 2×2 tile at quantity 0, lime round plates). SQLite has six rows in `brickvault_b5ebd8/inventory/dev` / `brickvault_b5ebd8/inventory.batches`.

### 6. The bundle and the public site — 16:07:33Z

The subject wrote `site/index.html` (12,553 bytes) and `PUT` it as a zip to `/schema/…/0.0.1-dev/bundle`. The response was `ok` with `files: 1` and that size. `GET http://dev.inventory.brickvault_b5ebd8.localhost:3300/` returned `<title>Brick Vault Inventory</title>`. `GET` of `/data/batches/list` on that host, with no token, returned `True 6`.

A token-less `PUT` of batch `2438ecd5-023e-414f-8c12-303b54788fb0` set `quantity` to `1` and `unit_price` to `null`, then a second `PUT` restored `quantity` `0` and `unit_price` `0.07`. Both were `ok`, and `updated_by` was `public`. `DELETE` of the same id returned:

```
{"ok":false,"error":"you do not have access to this resource"}
```

### 7. Validation, then the published site — 16:07:41Z

A token-less `POST` to the dev host with `quantity` `"lots"` and `condition` `"mint"` returned:

```
{"ok":false,"error":"validation failed","diagnostics":[{"severity":"error","kind":"type_mismatch","path":"quantity","message":"field 'quantity' expected type 'integer', got 'string'"},{"severity":"error","kind":"invalid_option","path":"condition","message":"field 'condition' must be one of [new, used], got 'mint'"}]}
```

`POST /config/version/…` with `{"version":"0.0.1","from":"0.0.1-dev"}` copied the draft. `POST /config/site/…` created site `www` pinned to `0.0.1` and dataset `dev`. `GET http://www.inventory.brickvault_b5ebd8.localhost:3300/` returned the same title, and a token-less list printed all six batch codes. `GET http://localhost:3300/` was still the JSON 404 with `see`.

The subject stopped at 16:07:51Z. It said the page was checked with HTTP and had not been opened in a browser. The summary matches the requests above. It says `www` is published and further changes go through a new version copy. It also says anyone who opens the page can view, add, and edit batches, because "a hosted Loco page can't carry a login." That sentence is the guide's framing. The guide steers every outsider to a publicly writable page, and that is the only hosted-editing path this run found.

### 8. Observer check of the page, after the subject stopped

The subject was `done` and was not prompted again. The observer opened `http://www.inventory.brickvault_b5ebd8.localhost:3300/` in a browser. The heading was "Brick Vault Inventory". The summary was 6 batches, 4,282 pieces, $205.90 stock value, and 1 out of stock. The table showed all six batches, with −/+ and Edit on each row. The only console error was `404` for `/favicon.ico`.

Clicking + on `B-2026-005` (the tile at quantity 0) changed that cell to 1, the piece count to 4,283, the stock value to $205.97, and out of stock to 0. That click is the observer's, so the throwaway row was left at quantity 1. The subject had left it at 0.

## Comparison with 01

Trial 01 ([`outsider-01.md`](outsider-01.md)) is the same task, the same kind of server, and the same rules. The text of the prompt matches, including the origin.

| | Trial 01 | Trial 02 |
|---|---|---|
| Signup without a hint | No. Idle from 16:21:45Z. One hint at 16:37:23Z (`Signup is POST /auth/users`). `POST /auth/users` then returned `401` `invalid credentials` for `lego-reseller` and `brickstock-<hex>`. No account. | Yes. Account `brickvault_b5ebd8` at 16:06:24Z, 33 seconds after the prompt. No hint. |
| Schema | Not reached. | Yes. Collection `batches`, nine fields, public permission set, manifest, at 16:06:32Z. |
| Records | Not reached. | Yes. Six batches at 16:06:40Z. |
| Published UI | Not reached. | Yes. Draft bundle at 16:07:33Z. Site `www` on version `0.0.1` at 16:07:41Z. The page is served and its data endpoints answer token-less list and update over HTTP. (Page rendered and + verified by the observer after the subject stopped, §8.) |
| Hints | 1. The subject asked for a second and did not get one. | 0. |
| Time | Commands from 16:21:19Z to 16:21:45Z, then 15 minutes idle, then about 26 seconds after the hint. Stopped 16:37:49Z. | One turn, 2 minutes 0 seconds (16:05:51Z to 16:07:51Z). |

What changed on the server between the commits (`d81e6ba` then, `b93ec54` now) is what the subject used:

- The apex `404` now sets `see` to `/.well-known/loco.json`. Trial 01's `GET /` was `no such endpoint: /` with no pointer, and the subject stopped guessing. This subject followed `see` on the first reply.
- The discovery document names signup (`POST /auth/users`, the handle rule, `400` for a bad handle, `409` when the handle is taken), project create, schema writes, record writes, the bundle upload, and the three-label site URL. Trial 01 had none of that. `GET /llms.txt` was `no such endpoint` then.
- The handle this subject chose matches the rule the document states (`brickvault_` and hex). Trial 01's handles contained `-`, and the server answered `401` `invalid credentials`.

`#150` (org create and member-add errors, PR #152) did not come up. The subject never called `POST /config/org`.

## Ranked proposed fixes

The orchestrator files follow-up issues from this list. This run was not stopped. The items are what the session still ran into, or the walls trial 01 listed that this run still did not need, in that order.

1. **The URL in the prompt still does not serve the app.** `GET http://localhost:3300/` returned `404` `{"error":"no such endpoint: /","ok":false,"see":"/.well-known/loco.json"}`. The subject published at `http://www.inventory.brickvault_b5ebd8.localhost:3300/`, which is the pattern the discovery document gives, and the document already says the apex stays JSON and names **#145**. #145 is the open issue (replace the three-label host with a one-label registry). This did not stop the task.

2. **MCP tools and a `loco` CLI, as in [ROADMAP.md](../../ROADMAP.md) item 2.** The subject did not look for one. The discovery document says there is no `loco` command and tells the reader not to look for a CLI. No open issue on 2026-10-09. The open issue list has no MCP, CLI, or starter item. The work remains ROADMAP item 2 and the sketches in [FUTURE_IDEAS.md](../../FUTURE_IDEAS.md).

3. **#137, installable packages.** Not encountered. The manifest dependency list is empty. A non-member still cannot depend on `loco/bricklink`. #137 is the open issue.

4. **The discovery document exceeds common agent inline-output limits (43KB).** No issue exists. `GET /.well-known/loco.json` was about 43KB. Claude Code wrote it to a tool-result file under its session directory and showed a 2KB preview. The follow-up `cat` of that path is what auto mode refused (§2). The preview included the guide's first step and did not include signup. Those 16 seconds are the cost on this harness. A smaller entry point, such as the short step list on `/llms.txt` or at the top of the document with the long reference split out, would have kept the next step in the inline reply. A harness with no Read tool could have stopped at the refusal.

5. **A starter for a task the guide does not already walk through.** No open issue. This run followed the guide's own parts-inventory example, so it leaves the starter question open. Trial 03 should use a task that is not inventory-shaped and not in the guide: a store that depends on `loco/bricklink@1.0.0` and reads `store:orders` (that reaches #137), or a declared action, or a member-login page.

The auto-mode refusal in timeline item 2 is Claude Code (Opus 5.5), the same class of stop as trial 01's safety classifier. Read of the same file was allowed, and the subject continued. There is nothing to file on the Loco repo for that refusal.
