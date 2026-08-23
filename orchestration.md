# Orchestration

How Grok and Claude take turns running Loco development. GitHub is the ledger (issues, PRs, reviews). Herdr is the runtime (panes, worktrees, agent start/prompt/wait). This file is the process.

The orchestrator **owns** this file. Update **Current term** at the start of every term, and the log at the end of any cycle that changed process.

Product conventions stay in [`CLAUDE.md`](CLAUDE.md). Filing rules stay in [`CONTRIBUTING.md`](CONTRIBUTING.md). “Where we left off” stays in [`HANDOFF.md`](HANDOFF.md). Do not duplicate those here.

## Starting a term

Cold start. You already have `CLAUDE.md`. Then:

1. Read **Current term** above. If you are not the named orchestrator, do not take the chair unless Ben hands it to you in this file.
2. Read `HANDOFF.md` and `ROADMAP.md`. List the backlog: `gh issue list --repo loco-hq/loco --limit 50`.
3. Do **not** write application code. Pick a shippable issue (see `CONTRIBUTING.md`). Do the still-real check (Cycle step 3). If you spawn, spawn the **other** vendor as implementer via herdr (recipes below).
4. Only `eval "$(python3 scripts/agent-github/token.py env grok)"` (or `claude`) when *you* file issues or comment as that bot. The implementer uses their own vendor env in their pane.
5. If `token.py` errors, stop and tell Ben. Credentials are `~/.config/loco-hq/apps/loco-{grok,claude}.{pem,json}` on this machine.

Ben merges. You do not.

## Current term

| Field | Value |
|---|---|
| Orchestrator | grok |
| Started | 2026-08-23 |
| GitHub identities | Installed on `loco-hq/loco` only. `loco-grok` app 4694991 / install 156025732. `loco-claude` app 4695030 / install 156026353 |
| Org | `loco-hq` (free). Repo is `loco-hq/loco` (public) |
| Merge | Human (Ben) in phase 1. `main` requires a PR + 1 approving review; stale reviews dismissed; admins included. **No GitHub Actions yet** — merge on an approval plus tests the reviewer ran locally. |
| Cadence | One issue at a time. Do not fan out; vendor plan limits. Herdr has no usage/quota API. |
| Active work | Product: issue #2 → PR #21 (`loco-claude[bot]`), `loco-grok[bot]` approved, waiting on Ben. Process: first-cycle lessons PR (this file). |
| Next | After Ben merges #21: teardown the issue-2 worktree and reviewer pane. Then pick the next issue. Do not spawn a product implementer while a product PR is waiting on Ben. |

## Why this exists

Two vendors in the loop so one model does not rubber-stamp itself. The typing of code is delegated. Judgment is split: implementer writes, the other vendor reviews, the orchestrator resolves disputes, the human still owns `main` until that is deliberately graduated.

Without separate GitHub identities, every commit and review looks like Ben, self-approval is impossible, and branch protection is theatre.

## Roles

| Role | Who | Job |
|---|---|---|
| Human | Ben | Merge to `main` (phase 1). Break ties the orchestrator will not. Own billing, org, and App private keys. |
| Orchestrator | Grok **or** Claude, rotating by **term** | Read process + backlog. Pick work. Spawn implementer and reviewer via herdr. Keep this file honest. Resolve implementer/reviewer disputes. Do **not** write the feature. |
| Implementer | The other vendor, by default | One GitHub issue → one branch → one PR. Follow `CLAUDE.md`. |
| Reviewer | The vendor that did **not** implement | Review the PR **on GitHub** (`gh pr review`). Comment, request changes, or approve. Escalate disputes to the orchestrator; do not privately “agree to disagree” in a pane. |

A **term** is one orchestrator sitting the chair until they hand it off in this file. A term may cover many issues.

**Invariant:** implementer vendor ≠ reviewer vendor. The orchestrator may be the same vendor as the reviewer (the default shape) or a third live pane. The orchestrator is never the implementer for a **product** issue.

**Exception:** process-doc PRs (this file, and close-the-loop `HANDOFF.md` notes) are authored by the sitting orchestrator’s vendor and reviewed by the other. That is how this file moves; it does not go on `main` uncommitted and it does not fold into a product PR.

OpenAI, if added later, is a third vendor with a third GitHub App. Same rules: reviewer is not the implementer.

## Cycle

One issue at a time. Parallel issues are allowed only when Ben says the plan budget can take it **and** the issues do not share files. This term is serial.

1. **Orient.** Orchestrator reads this file, `HANDOFF.md`, `ROADMAP.md`, and `gh issue list`. Does not invent a parallel tracker.
2. **Pick.** Choose a shippable issue (see `CONTRIBUTING.md`). If the work is not an issue yet, file one first. The implementer does not start from a chat message.
3. **Still-real.** Before spawning, open the issue’s cited `file:line` (about thirty seconds). If the change is already on `main`, comment and close the issue. If it is half-done, say that in the implementer prompt — do not pretend it is greenfield. If the ticket is wrong, too ambiguous to implement, or you disagree, **do not spawn**. Comment on the issue with what you think instead; escalate to Ben when it is a product call.
4. **Spawn implementer.** Herdr worktree + `agent start` of the **other** vendor (see [Herdr recipes](#herdr-recipes)). Prompt names the issue number, the GitHub identity they must use, and that they open a PR rather than pushing `main`.
5. **Wait.** `herdr agent wait` until idle/done/blocked. Blocked (approval UI, missing token) is the orchestrator’s problem, not the reviewer’s.
6. **Spawn reviewer.** Other vendor (if the implementer was the default “other”, the reviewer is this orchestrator’s vendor — prefer a fresh pane so this pane stays a process role). Reviewer reads the PR via `gh`, posts a **GitHub** review, not a herdr sidebar comment as the record of decision. The prompt must include the implementer worktree path so they can run tests there instead of dirtying `main`.
7. **Address or escalate.**
   - Request changes → implementer pushes, reviewer re-reviews. Repeat.
   - Dispute (implementer believes the review is wrong) → both comment on the PR @-mentioning the question; orchestrator decides on the PR, in public.
   - Approve → stop. Do not merge in phase 1.
8. **Human merge.** Ben merges when the review is an approval and he agrees. Orchestrator does not merge. **There is no GitHub Actions CI yet.** Missing checks are not a block; the reviewer runs the relevant tests locally and says so on the review. When CI exists, this step also requires green checks — do not invent that requirement now.
9. **Close the loop.**
   - Comment on the PR: implementer, reviewer verdict, waiting on Ben. That comment is the cycle record.
   - Product state → `HANDOFF.md`. Process → this file, as its **own small PR** (orchestrator’s vendor authors, the other vendor reviews). Do not leave these dirty on `main`. Do not fold them into the product PR.
   - **Keep** the implementer worktree and reviewer pane until Ben merges (requested-changes may resume).
   - **After merge:** close the implementer workspace (`herdr workspace close <id>`), remove the checkout if it remains (`herdr worktree remove --workspace <id>`), close the reviewer pane (`herdr pane close <id>`). Do not close panes you did not create.
   - The product issue closes via its PR. Hand off the term when done sitting.

Do not push to `main`. Do not approve your own PR. Do not close a review-requested PR because the pane got bored. Do not spawn the next product issue while one is waiting on Ben.

## GitHub identities

Repo is `loco-hq/loco` (transferred from `humandad/loco` on 2026-08-23). The old URL redirects.

### What we want each agent to do with `gh` / `git`

| Action | GitHub App (org install) | GitHub App (personal-account install) | Machine user (collaborator PAT) | Act as `humandad` |
|---|---|---|---|---|
| Distinct bot identity (`name[bot]`) | yes | yes | no (looks like a person) | no |
| Create issues, comment | yes | yes | yes | yes, as Ben |
| Push a branch | yes | yes | yes | yes, as Ben |
| **Create a PR** | yes | **often no** — App is not a collaborator, and GitHub Apps cannot be invited as collaborators on a personal repo | yes | yes, as Ben |
| Review / request changes | yes | yes | yes | yes, as Ben |
| Approve someone else’s PR | yes (cannot approve its own) | yes | yes | cannot approve “own” work if it also opened the PR |
| Merge | yes, if permitted | yes, if permitted | yes | yes |
| Short-lived tokens | installation token, 1h | same | no (PAT) | no |
| Consumes a GitHub seat | no | no | yes, on an org; free collaborator on a personal repo | — |

`gh` **does** work as a GitHub App. Set `GH_TOKEN` to an **installation access token** (not the App JWT, not a PAT). Same token as the git HTTPS password (`x-access-token:TOKEN`). Tokens expire in an hour; do not put them in the git credential store.

### Decision: org + two Apps, human merges

Locked 2026-08-23: create org `loco-hq`, transfer `loco`, two Apps, Ben merges in phase 1. GitHub Apps are the blessed bot identity. Machine-user accounts are fake people, long-lived secrets, and against the spirit of GitHub’s ToS. The catch was **PR creation on a personal repo** — the org install removes it.

**Do this:**

1. ~~Create free GitHub organization `loco-hq`.~~
2. ~~Transfer to `loco-hq/loco`.~~ Remote is `git@github.com:loco-hq/loco.git`.
3. Register **two** GitHub Apps, owned by the org, installed **only** on `loco`:
   - `loco-grok` → `loco-grok[bot]`
   - `loco-claude` → `loco-claude[bot]`

   Create them in the GitHub UI (org Settings → Developer settings → GitHub Apps). PEMs live on the machine, not in the repo (`~/.config/loco-hq/apps/`).
4. Permissions, both Apps, repository only, webhook **off**:
   - Contents: read & write
   - Issues: read & write
   - Pull requests: read & write
   - Metadata: read
   - Checks: read (so a reviewer can see CI)
   - Workflows: read & write **only if** an agent must change `.github/workflows/` — add later, not on day one
5. ~~Branch protection on `main`.~~ Public as of 2026-08-23 so the free plan allows it. `main`: require a PR, 1 approving review, dismiss stale reviews, enforce for admins, no force-push, no App bypass. Ben still merges.
6. Agents mint an installation token and set git author so commits and the PR actor match:

   ```bash
   eval "$(python3 scripts/agent-github/token.py env grok)"    # or claude
   ```

   Identities:

   | Bot | git author |
   |---|---|
   | `loco-grok[bot]` | `320313737+loco-grok[bot]@users.noreply.github.com` |
   | `loco-claude[bot]` | `320314916+loco-claude[bot]@users.noreply.github.com` |

   `gh api user` 403s on installation tokens; that is expected. `gh issue` / `gh pr` work.

A **third** App (`loco-openai`) slots in later the same way. Do not share one App across vendors — then implementer and reviewer are the same GitHub actor and cannot approve each other.

### Rejected alternatives

- **One App, two installations.** Still one identity.
- **Stay on `humandad` and install Apps anyway.** Fine for comments and reviews on PRs a human opened. Breaks “agent opens the PR” unless we also add machine users, which is the worse half of both designs.
- **Machine users only.** Works on a personal repo today. Costs MFA/passwords/ToS. Use only as a fallback if we refuse to move the repo.
- **Agents keep using Ben’s `gh` auth.** Current state. Makes cross-vendor review unverifiable.

### Status

Org, transfer, Apps, installs, token helper, public visibility, and `main` protection are done. Agents must `eval "$(python3 scripts/agent-github/token.py env grok|claude)"` before any `git` / `gh` write. Do not fall back to Ben’s `gh` auth. An App cannot approve its own PR.

## Herdr recipes

Herdr kinds we use: `grok`, `claude`. (`codex` if/when an OpenAI subscription exists.)

Names are live aliases, `[a-z][a-z0-9_-]{0,31}`, unique among running agents. Reuse `impl` / `review` / `orch` so prompts stay short.

Create a worktree, start the implementer, give it the issue:

```bash
created=$(herdr worktree create --cwd /Users/ben/dev/loco \
  --branch "issue-N-short-slug" --base main \
  --label "issue-N" --no-focus)
pane_id=$(printf '%s\n' "$created" | jq -r '.result.root_pane.pane_id')

herdr agent start impl --kind claude --pane "$pane_id"
worktree=$(printf '%s\n' "$created" | jq -r '.result.worktree.path')
herdr agent prompt impl "$(cat <<'EOF'
You are the implementer for loco-hq/loco issue #N.
Read CLAUDE.md, the issue, and orchestration.md.
Before any git or gh write: eval "$(python3 scripts/agent-github/token.py env claude)"
Do not push main. Open one PR for this issue. Stop when the PR is up and tests you can run locally have passed.
There is no GitHub Actions CI yet; local tests are the proof.
EOF
)" --wait --timeout 1200000
```

`--timeout 1200000` (20 minutes) is the default for an implementer turn: cold `cargo test --workspace` exceeds 10 minutes. Raise it if the wait returns timeout while the pane is still working.

Reviewer sits in a sibling pane on the **main** checkout. Tell them the worktree path so tests run there, not in `main`:

```bash
split=$(herdr pane split --current --direction right --cwd /Users/ben/dev/loco --no-focus)
review_pane=$(printf '%s\n' "$split" | jq -r '.result.pane.pane_id')

herdr agent start review --kind grok --pane "$review_pane"
herdr agent prompt review "$(cat <<EOF
You are the reviewer for PR #M (issue #N). Implementer was Claude.
eval \"\$(python3 scripts/agent-github/token.py env grok)\"
Read the diff with gh (not the herdr sidebar).
Implementer worktree (run tests here, do not dirty main):
  ${worktree}
There is no GitHub Actions CI. Missing checks are not a block. Run the acceptance tests in that worktree and say what you ran on the review.
Post a GitHub review: approve, comment, or request changes.
Do not push code. If you and the implementer disagree, comment on the PR for the orchestrator; do not merge.
EOF
)" --wait --timeout 600000
```

`agent start` does not create layout — the pane must already exist and be at a shell prompt. `agent prompt --wait` returns on idle/done/blocked; blocked means go look at the pane. Orchestrator `agent read` does **not** mark a `done` agent as seen.

## Review bar

The reviewer is a second vendor, not a linter. On the PR they must actually say one of: approve, request changes, comment.

Request changes when: behavior is wrong, tests that should exist don’t, `CLAUDE.md` conventions are broken, or the PR is larger than the issue.

Comment (not block) when: naming nits, optional follow-ups. File a follow-up issue rather than growing the PR.

Approve when: the issue’s acceptance is met, you ran the tests that prove it (or the implementer did and you re-ran the ones that matter), you would be willing to have Ben merge it, and you are not the implementer. Empty CI on the PR is expected until we add GitHub Actions.

## Disputes

Posted on the PR, not in a herdr pane.

1. Implementer replies to the review with why the change is wrong or out of scope, and what they propose instead.
2. Reviewer replies: stand, or drop to a comment / follow-up issue.
3. If still stuck, both @ the orchestrator (or the orchestrator notices `agent wait` idle with an open requested-changes review). Orchestrator comments the decision. That decision is binding for this PR.
4. If the orchestrator will not decide (product call, merge policy, identity/secrets), escalate to Ben.

Do not resolve a dispute by switching vendors so someone friendlier reviews.

## What the orchestrator updates

| File | When |
|---|---|
| **Current term** in this file | Start of term; any identity/process change |
| Log below | End of a cycle that taught us something about process |
| `HANDOFF.md` | Product “where we left off” changed |
| GitHub issue / PR | Status of the work itself |

Those file edits ship as a **small process PR**, not as uncommitted changes on `main` and not as extra commits on the product branch. Do not turn this file into a changelog of every merged PR.

## Log

- **2026-08-23** — Org `loco-hq` created. Repo transferred to `loco-hq/loco` and made public. Apps `loco-grok` / `loco-claude` installed on that repo only. Token helper is `scripts/agent-github/token.py`. `main` requires a PR and 1 approving review (admins included). Ben merges.
- **2026-08-23** — First product cycle. Claude implemented issue #2 in a herdr worktree (`issue-2-stop-auto-create-person`); opened PR #21 as `loco-claude[bot]`. Grok reviewed from a sibling pane and approved as `loco-grok[bot]`. Recipe worked: worktree + `agent start`/`prompt --wait`, tokens from `token.py`, no self-approval. Orchestrator did not merge.
- **2026-08-23** — First-cycle process lessons (this edit): still-real check and the right to not spawn; process docs are their own PR; keep worktree/reviewer pane until merge then teardown; reviewer prompt includes the worktree path; implementer wait 20 minutes; no CI yet, local tests are the bar; serial cadence while on basic vendor plans. Herdr API has no plan/usage fields.
