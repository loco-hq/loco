# Filing work on Loco

This is the contract for turning reviews into GitHub issues. Implementation conventions live in [`Claude.md`](Claude.md). There is no project board; issues and milestones are the backlog.

Repo: `humandad/loco`. Use `gh` against that remote.

## Who does what

One orchestrator reads the source documents, deduplicates, creates **milestones**, then files **issues**. Do not file issues before milestones exist. Do not invent a parallel tracker.

Current review sources:

- [`docs/architecture_review/2026-08-23_claude.md`](docs/architecture_review/2026-08-23_claude.md)
- [`docs/architecture_review/2026-08-23_grok.md`](docs/architecture_review/2026-08-23_grok.md)

Also read [`ROADMAP.md`](ROADMAP.md) and [`HANDOFF.md`](HANDOFF.md) so the backlog does not fight existing intent.

## Milestones first

A milestone is a **closable batch**, not a category. Area and type are labels.

Create 2–4 milestones after synthesizing the reviews, before filing issues.

```bash
gh api repos/humandad/loco/milestones \
  -f title='short goal' \
  -f description='What "done" means for this batch.'
```

Rules:

- Title is a goal (`Auth that cannot leak`, `Schema view and data plane agree`), not `Phase 0` unless that phrase is still the honest name of the batch.
- Description states the done condition. When every issue in it is closed, close the milestone.
- Unscheduled work has **no** milestone. Do not create an icebox milestone.
- If you need more than four open milestones, you are categorizing. Use labels instead.
- Rename or replace a milestone if the batching was wrong; do not keep empty leftovers.

List what you created, then file into those titles:

```bash
gh api repos/humandad/loco/milestones --jq '.[] | {number, title, open_issues, state}'
```

## One issue = one shippable change

Merge overlapping review findings into a single issue. Both reviews calling out `require_collection` is one issue, not two.

Yes: `Gate /auth/users to self or org owner and add Hurl coverage.`
No: `Auth is a prototype.` / `Fix architecture section 2.`

Search before creating:

```bash
gh issue list --repo humandad/loco --state all --limit 50
gh issue list --repo humandad/loco --search 'require_collection'
```

Title: imperative, specific, no review-author prefix.

Body: use the Task template (`.github/ISSUE_TEMPLATE/task.md`). Required sections are **Problem**, **Evidence**, **Proposed change**, **Acceptance**, **Source**. Point at `file:line` and the review heading. Do not paste the review.

```bash
gh issue create --repo humandad/loco \
  --title "Gate /auth/users on self-or-owner" \
  --label "p0,type:security,area:auth" \
  --milestone "Auth that cannot leak" \
  --body-file path/to/body.md
```

`--body-file` must be the markdown sections only (no GitHub template front matter).

Optional: `--blocked-by` / `--blocking` when work cannot start until another issue lands. Do not build a large issue graph; a couple of blockers is enough. Prefer independent issues.

## Labels

Every issue gets **one priority**, **one type**, **one or two areas**. Use these names, not the GitHub defaults (`bug`, `enhancement`, …).

| Kind | Labels | Pick |
|---|---|---|
| Priority | `p0` `p1` `p2` | `p0` blocking (security, data loss, broken invariant). `p1` high leverage, next after p0. `p2` real but not this batch. |
| Type | `type:security` `type:bug` `type:architecture` `type:product` `type:chore` | Security beats bug. A missing capability is `type:product`, not `type:architecture`, unless the work is changing a trait or storage model. |
| Area | `area:auth` `area:schema` `area:data` `area:lake` `area:codegen` `area:studio` | Where the code changes. Two areas only when the fix necessarily spans them (e.g. `area:data` + `area:schema` for collection visibility on `/data`). |

`needs-triage` means you filed without a priority. The orchestrator should almost never need it: set `p0`/`p1`/`p2` when creating.

Priority vs milestone: `p0` can sit in a later milestone if it is blocked; a milestone can mix `p0` and `p1`. Do not use priority as a substitute for a milestone, or the other way around.

## After filing

Print the backlog grouped by milestone so a human can scan it:

```bash
gh issue list --repo humandad/loco --limit 100 --json number,title,labels,milestone \
  --jq 'group_by(.milestone.title // "unscheduled")[] | {milestone: .[0].milestone.title // "unscheduled", issues: map({number, title, labels: [.labels[].name]})}'
```

Do not close, edit, or retitle issues from a later agent unless you are the orchestrator correcting a filing mistake.
