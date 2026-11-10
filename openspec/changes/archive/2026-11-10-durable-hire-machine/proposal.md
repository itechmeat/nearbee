# Proposal: durable hire machine

## Why

Stage S3 of `docs/stages.md` (plan items 8 to 12, plus the items carried
from S2). S2 serves only first-time hires, one at a time, blocked on each
run; it ignores rework, redo, disputes and acceptance, checks no brief,
spends without a ceiling and drops attachments. The live hires of
2026-11-10 also showed that `nearbee list` cannot change the market agent
with an agent token. S4 and S5 list real example agents on top of this
machine, so it has to survive restarts, cancellations and reopened work
without a duplicate write. See `brainstorm.md`.

## What Changes

1. `nearbee serve` becomes a per-round state machine: every round lists
   each agent's assignments once, reconciles every journaled hire with its
   row, and advances each open execution by at most one bounded step; no
   step waits for a run to end.
2. Hire lifecycle: generations per hire (first-time work, each rework,
   each dispute redo), closing on `accepted`, `cancelled` or
   `dispute_resolved`; `disputed` freezes the hire.
3. Rework and redo run as continued apb runs with the original brief, the
   newest buyer message as the untrusted `feedback` param, the remaining
   budget and a cleared `out/`.
4. Intake before start: optional hard limits from the listing file, the
   scope check through `apb decide is`, the cost admission, decline texts,
   the one-hour rule.
5. Cost protection: per-hire cumulative ceiling from the award amount (or
   the listing price for a zero-escrow owner test run) as the run budget,
   a wall-clock stop, a daily loss limit, `protection: off`.
6. Stopping a run aborts it (`apb stop`) and waits for a terminal status;
   an ambiguous start is settled before any second start, and a stray run
   of the hire is aborted.
7. Attachments: authenticated, origin-checked, streamed download with size
   and sha256 checks into the hire worktree, passed as the `inputs` param.
8. Private per-generation snapshots of the run request and of each
   submission next to the journal; the journal keeps digests.
9. A lock file for `serve` and `cleanup`; `nearbee hires`; `nearbee
   cleanup` with the deletion inventory and `--dry-run`.
10. **BREAKING**: `nearbee list` makes no market writes. It materialises
    and installs the playbook, records the listing, reads the pricing
    plans and prints what the owner sets in the web app.
11. Fixes found live on 2026-11-10 (numeric playbook version, the apb
    trust prompt wording, `apb export --out`, the short-SLA margin, a
    `version: null` first observation) land with tests.
12. Carried from S2: the fixture capture example fixed and the live
    fixtures recorded (task 3.4, with the token moved out of `.env`); the
    owner tasks 8.2, 8.3 (x402) and 9.1 (billed Cloud smokes) stay owner
    gated.

## Capabilities

### New Capabilities

- `hire-intake`: whether a hire is taken: hard limits, scope check, cost
  admission, daily loss limit, decline texts, the one-hour rule.
- `hire-attachments`: how buyer files reach a run: download, checks,
  layout, the `inputs` param.
- `hire-retention`: what Nearbee keeps per hire, how `nearbee hires` shows
  it and how `nearbee cleanup` deletes it.

### Modified Capabilities

- `hire-serving`: per-round machine, lifecycle, generations, rework and
  redo, abort, ambiguous start, cost budget, lock file, version check on a
  pending first observation.
- `hire-journal`: generations, private snapshots, read errors.
- `listing-publish`: listing file keys `limits`, `scope`, `protection`;
  publishing makes no market writes and reports the web-app settings.
- `market-client`: attachment download, the row fields the core keeps,
  a time budget for read retries.
- `apb-runner`: abort, optional budget, usage by basis and budget pause,
  `decide` through the CLI.
- `nearbee-config`: `limits.daily_loss_usd` and
  `retention.after_acceptance_days`.

## Impact

Crates: `nearbee-core` (serve, journal, listing, new intake, attachments
and retention modules; traits `Market` and `Runner` gain methods),
`nearbee-market` (attachment download, row fields, read time budget,
capture example), `nearbee-apb` (abort, decide, usage basis, optional
budget), `nearbee` (commands `hires`, `cleanup`, `list` without market
writes, the lock). The seam rule holds: the core gets new trait methods and
stays free of market and apb knowledge. New dependency: none expected
(`rustix` already provides `flock`; `url` parsing through `reqwest::Url`).
Outside systems: the market (owner's draft and owner's test hires only),
Nearbee's apb instance.

## Out of scope

1. Parallel runs (needs a git worktree per hire; S4).
2. Output file uploads in the deliverable (S4, with `findings.json`).
3. Buyer questions, a start message, push notifications, webhooks
   (D-011).
4. Third-party briefs: D-023 holds until apb releases the external-data
   flag.
5. Listings beyond the draft, tiers, evidence (S4, `docs/stages.md`).
6. Deleting apb run artifacts (no apb route at 1.0.0-beta.6; reported as
   not covered).
