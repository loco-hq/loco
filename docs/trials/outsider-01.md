# Outsider trial 01

A fresh Claude Code session, with no Loco checkout and no `CLAUDE.md`, was asked to build a small inventory app on a throwaway Loco server. It did not sign up, and it did not create a schema, records, or a hosted UI.

The transcript is [`outsider-01-transcript.md`](outsider-01-transcript.md). Passwords are redacted there. This file is the observer's account.

## Setup

- Date: 2026-10-08. Server commit `d81e6baae5222be221a2ead91a1154f1dce88041` (this worktree, `issue-144-outsider-trial`).
- Binary: `target/debug/loco-apps`, built in this worktree. Process id 29871. Bound `0.0.0.0:3300` only.
- `LOCO_ROOT` and `LOCO_DB_PATH` were under a new temp directory. `schemas/types` (14 files) and `schemas/seed` (84 files) were copied in before start. `LOCO_AUTH_AUTO_CREATE` was unset in the process environment. `LOCO_SECRET_KEY` and `LOCO_DEFAULT_SITE` were unset.
- Boot log: seeded `brickos/inventory`, `brocksbricks/orders`, `loco/bricklink`, `loco/core`, `loco/demo`, and `loco/studio`. Then `LOCO_SECRET_KEY is not set; PUT /config/secret will return 503`, SQLite at the temp `loco.db`, and local auth under that root. The auth adapter's usual seed wrote `alice`, `bob`, and the `loco` org into that temp auth directory. The subject never logged in as any of them.
- A request with no token, `GET /auth/me`, returned `401` `{"ok":false,"error":"missing auth: use Authorization: Bearer <token> header"}`.
- Ben's server on port 3000 was not used. Nothing in this trial bound that port.
- Subject: Claude Code v2.1.294, Opus 5.5, started with `herdr agent start subject --kind claude --pane w1A:p2 -- --permission-mode auto` in an empty temp directory outside the repository. The flag was accepted. The status line read `auto mode on`.
- Before the session would take a prompt, the pane blocked on the workspace trust check (`Yes, I trust this folder`). The observer selected that line and confirmed with Enter. That was the only dialog. It records trust for this temp directory in the local Claude settings. No "always allow" choice was offered or taken. After the pane was closed, the observer deleted that path's entry from `~/.claude.json` with a script and rewrote the file twice. Aside from that deletion, the file matched Claude Code's latest backup.

## Task

Sent at 2026-10-08T16:21:19Z, verbatim:

> Build a small inventory app on this Loco server for a LEGO reseller: batches of parts with quantities. Give it a schema, seed a few records, and publish a web UI that lists and edits them, hosted by Loco.
>
> The server is at http://localhost:3300. You must sign up for your own account. You must not read the loco-hq/loco GitHub repository or any Loco source or docs — only what the server itself answers and general web knowledge.

The subject was idle from 16:21:45Z until 16:37:23Z. That is the one hint, sent verbatim:

> Signup is POST /auth/users. Continue from that response. Still do not read the loco-hq/loco repository or any Loco source or docs.

No second hint was sent. The subject stopped again at 16:37:49Z and asked for another. The working directory was still empty. No new account file was written.

## Timeline

### 1. The origin says it has no API — 16:21:23Z

`GET /` and `GET /api` both returned `404`:

```
{"ok":false,"error":"no such endpoint: /"}
{"ok":false,"error":"no such endpoint: /api"}
```

On this process the apex is API-only because `LOCO_DEFAULT_SITE` is unset, and the fallback for a path the routers do not own is that sentence (`handlers/hosting.rs`). The subject treated it as "this server will not describe itself."

### 2. Ordinary discovery paths are the same 404 — 16:21:26Z

One shell loop requested `GET` of `/help`, `/docs`, `/openapi.json`, `/.well-known/openapi.json`, `/v1`, `/health`, `/api/v1`, `/signup`, `/auth/signup`, `/llms.txt`, `/robots.txt`, `/swagger.json`, `/_`, and `/index`. Every body was `{"ok":false,"error":"no such endpoint: <path>"}`.

### 3. The real prefixes look missing, and OPTIONS is empty — 16:21:29Z

A second loop included `GET /auth`, `GET /schema`, `GET /signup`, `GET /users`, `GET /.well-known/loco`, and `GET /collections`, among others. Same body, including:

```
{"ok":false,"error":"no such endpoint: /auth"}
{"ok":false,"error":"no such endpoint: /schema"}
```

`GET /auth/users` would have been `405` with `Allow: POST`. The subject never requested that path in this turn. `GET /auth` and `GET /schema` are reserved prefixes, and a request that does not match a route under them falls through to the same `no such endpoint` string as `/help`.

`OPTIONS /` returned `200` with `access-control-allow-methods: *`, `access-control-allow-headers: *`, `access-control-allow-origin: *`, and `content-length: 0`.

### 4. No CLI, and a further guess is cut off — 16:21:34Z to 16:21:36Z

`which loco` printed `loco not found`. `POST /` with `{}` returned `{"ok":false,"error":"no such endpoint: /"}`. A loop of `GET` and `POST` against `/v1/signup`, `/v1/users`, `/graphql`, `/sql`, `/query`, `/meta`, `/info`, `/version`, and the other paths in the transcript printed `404 404` for each.

The next model response was withheld. The pane showed:

> Opus 5.5's safeguards stopped the response above · continuing once with that noted

The following user turn, injected by Claude Code, said:

> Your response above was stopped by a safety classifier — this is not a tool or API error. The rest of it was withheld, and tool calls in it that had not finished did not run. Do not produce that content again, even reworded.

The fourth command had already finished. The withheld continuation is what the classifier stopped. The subject had not yet tried `POST /auth/users`.

### 5. The subject stops and asks for a path — 16:21:45Z

It said it would not keep guessing endpoint names. It listed what it knew (the 404s, the empty OPTIONS response, no `loco` binary, empty directory) and asked for one of: a starting path, permission to look for a CLI or client on a package registry, or some other hint from the task setup. The original prompt already allowed general web knowledge. The subject still asked before searching.

The prompt line then showed an unsent suggestion, `try GET /.well-known/loco.json for discovery`. It was not submitted. `GET /.well-known/loco` and `GET /.well-known/openapi.json` had already returned `no such endpoint`.

The session stayed idle until the hint at 16:37:23Z.

### 6. Signup names its missing fields, in two shapes — 16:37:26Z to 16:37:30Z

| Request body | Status | Body |
|---|---|---|
| `{}` | 422, `text/plain` | `Failed to deserialize the JSON body into the target type: missing field `username` at line 1 column 2` |
| `{"username":"lego-reseller"}` | 422, `text/plain` | `... missing field `name` ...` |
| `{"username":"lego-reseller","name":"LEGO Reseller"}` | 400, JSON | `{"ok":false,"error":"password is required"}` |

The subject followed those messages and added each field.

### 7. A filled signup body returns 401 — 16:37:34Z and 16:37:38Z

`POST /auth/users` with `username` `lego-reseller`, `name` `LEGO Reseller`, and a generated password returned:

```
HTTP/1.1 401 Unauthorized
{"ok":false,"error":"invalid credentials"}
```

The retry used `username` `brickstock-` plus three random hex characters (the command is `brickstock-$(openssl rand -hex 3)`). Same `401` and the same body.

Both usernames contain `-`. `LocalAuthAdapter::create_user` returns `AuthError::InvalidCredentials` when `is_valid_handle` fails (`loco-apps/src/auth/local.rs`). A handle is non-empty, not the public username, has no `/`, and is `[a-z0-9_]` starting with a letter or `_`. The HTTP mapping of that error is `401` `invalid credentials`, which is also the failed-login string. The subject wrote that the second `401` meant it was not a name clash, and stopped. The handle rule is the check that failed. `/config` names use the same charset and reject a bad one with `400` and the rule in the body (`http/names.rs`, shipped with #82). Signup does not.

### 8. The subject stops again — 16:37:43Z to 16:37:49Z

`GET /auth/users` returned `405` with `allow: POST` and an empty body. `POST` with `username` `x`, `name` `x`, and `password` `""` returned `400` `{"ok":false,"error":"password is required"}`.

The subject then wrote that a `401` on a body with every field it had been asked for means the server wants something further (an invite code, a header, or a password rule), and that it would not cycle through field names and headers. It asked for that extra field, header, or rule.

No account was created. No schema, records, or UI were attempted. The working directory was still empty.

## Ranked proposed fixes

The orchestrator files follow-up issues from this list. Observed blockers are 1–4. Items 5–7 are walls this run did not reach, in the order they would have mattered to the rest of the task.

1. **Say why signup rejected the handle.** `lego-reseller` and `brickstock-<hex>` are illegal handles, and the response is `401` `invalid credentials`. Return `400` with the same kind of rule `#82` already uses for `/config` names (`1-63` characters of `a-z`, `0-9`, and `_`, starting with a letter or `_`). `is_valid_handle` has no length cap today, so adopting that 1-63 limit is a behavior change. Missing JSON fields are a separate shape: `422` `text/plain` from the deserializer, then `400` JSON for an empty password. One JSON error that names the bad field or the rule would have let this subject continue. No open issue covers signup errors. #82 is closed and applies to project, dataset, site, and version names.

2. **Serve a discovery document from the process.** The subject searched `/openapi.json`, `/.well-known/openapi.json`, `/docs`, `/help`, `/llms.txt`, `/.well-known/loco`, and `OPTIONS /`. All of those are either `no such endpoint` or an empty CORS `200`. A small JSON document (route prefixes, the signup method and fields, and a pointer at how a project is created) is the fact this session asked for. No open issue. [ROADMAP.md](../../ROADMAP.md) item 2 is the scheduled interface work (MCP and a CLI). [FUTURE_IDEAS.md](../../FUTURE_IDEAS.md) sketches those two surfaces and does not specify an HTTP discovery document. Discovery is the smaller fix; item 4 is the larger one.

3. **Make a real prefix distinguishable from an unknown path.** `GET /auth` and `GET /schema` return `no such endpoint`, the same body as `GET /help`. `GET /auth/users` returns `405` with `Allow: POST`, which is a usable signal, and the subject only found it after the hint. A `404` on `/auth` or `/schema` that names the mounted routes would have ended the guessing. No open issue. This can ship as part of the discovery document in item 2.

4. **MCP tools and a `loco` CLI, as in ROADMAP item 2.** `which loco` was `loco not found`, and the subject asked permission to look for a client package. An installable CLI or an MCP server that can sign up, describe schema, write records, and publish a bundle would have replaced the URL scan. The safety classifier (below) fired because the subject was enumerating paths. No open GitHub issue turned up in a search of the issue list on 2026-10-08. The work is ROADMAP item 2, with the tool list in FUTURE_IDEAS.md under "MCP server" and "Loco CLI".

5. **Hosting the UI on the origin the operator handed out.** Not reached. `GET /` already returned `no such endpoint: /`, which is what the apex does with `LOCO_DEFAULT_SITE` unset. A bundle is served at `{site}.{project}.{account}.<listen-host>` ([docs/hosting.md](../hosting.md), "Sites as hosts"). On this server that is a name like `dev.inventory.legoreseller.localhost:3300`, and `http://localhost:3300/` would still be the JSON 404 after a successful publish. Closed #30 is the serving implementation. #145 replaces the three-label public name with a one-label registry. #107 is the later trial against the deployed server, which this issue already schedules, and it does not change what a localhost agent is told.

6. **A starter the agent can copy.** Not reached, and no open issue. A template only helps after the agent can find the API and sign up (items 1–4). The production binary also registers no action handlers, so a starter that needs server-side logic would still get `501`. This task's list-and-edit UI is `/data` CRUD and does not need a handler. No open issue covers a starter.

7. **#137, installable packages.** Not encountered. The task was a new app with its own schema. A later step that added `loco/bricklink` to the manifest would be a `400`, because a non-member cannot depend on that project. #137 is the open issue.

The classifier stop in timeline item 4 is Claude Code (Opus 5.5), not a Loco bug. The Loco-side remedy is item 2 or item 4, so the agent has a document or a tool and does not invent paths. There is nothing to file on the Loco repo for the classifier itself.
