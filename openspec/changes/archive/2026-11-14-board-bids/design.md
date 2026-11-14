# Design: board bids

See `proposal.md` for why. Facts are from the OpenAPI refreshed on
2026-11-14, the worker skill 0.67.0 and read-only measurements with agent
tokens on 2026-11-14 (`docs/market.md`, "Job board").

## Context

1. The board answers `{"jobs": [JobView]}`; a `JobView` is a `Job` plus
   `creator` (`accountId`, `displayName`) and `quoteState`. The job carries
   `jobId`, `creatorAccountId`, `title`, `description`, `tags`,
   `budgetAmount` (nullable decimal string), `budgetToken` (`USDC` or
   `USD`), `jobKind` (`standard`, `instant`, `competition`), `status`,
   `maxSlots`, `currentMaxSlots`, `minBuilderVerification`, `isTestRun`,
   `invitedAgentId` (a quote request), `requiresVerifiable` (required;
   a Nearbee bid never offers verifiable delivery, so such a job is
   skipped). It carries no deadline (only
   `entriesCloseAt` for competitions and `quoteDeadlineAt` for quotes) and
   no attachments (`GET /v1/jobs/{job_id}/attachments` lists them, and
   the bidder reads them before intake so the hard limits judge the bid as
   they will judge the award; the award row embeds them too).
2. An instant hire already creates an `accepted` bid with the proposal
   "Direct hire at the listed price", and its assignment carries `bidId`.
   A board award is the same: the bid turns `accepted` and an assignment
   with `bidId` appears in `GET /v1/agents/me/assignments`. The row shape
   is the one serve already reads, so the award path needs no new code;
   a test pins it.
3. The token cannot read the owner's verification badge, so the owner
   states it in the config (`owner_verification`).
4. `POST /v1/jobs/{id}/bids` refuses `own_job` (400): the owner cannot win
   their own job with their own agent. The demo poster is a second market
   account; the allowlist names it.

## Decisions

### 1. The `board` block lives in `nearbee.yaml`, keyed by handle

Bid policy is operational (what to search, how much, which posters), may
change during the demo, and must not re-materialise the playbook. The
listing file stays the contract of the listing; `nearbee.yaml` names the
handles that bid. A handle without a resolved record or credentials is
warned about each pass and skipped. Alternative rejected: a `board` field
in the listing file, which ties a demo toggle to `nearbee list`.

### 2. A second trait, `Board`, beside `Market`

`nearbee_core::board::Board` has `board_jobs(query, kind)`,
`pending_bids()`, `job_attachments(job_id)` and `place_bid(job_id,
request)`. `NearAgentMarket` implements it.
Adding the calls to `Market` would touch three fakes and the replay for
no use; the seam rule is kept (only `nearbee-market` knows the paths).

### 3. The bidder pass

`nearbee_core::board::pass(board, runner, ctx, settings, state, ...)`
returns a list of `BidEvent`s (matched, skipped with a reason code, sent,
refused, awarded, error) that serve prints. Order per listing:

1. Read `pending_bids()` (`status=pending` only: `status=all` holds an
   accepted bid per instant hire and grows for the agent's lifetime). A
   read failure ends the pass for the listing (logged); nothing is bid
   without knowing the open bids.
2. Settle the journal against the pending bids and the job ids of the
   listing's assignment rows from the same loop iteration's hire read
   (`PassInputs::assigned_jobs`; the agent token has no per-bid read): an
   open intent whose job has a pending bid or an assignment closes as
   `placed`; a journaled bid reads `pending`, `accepted` (an assignment
   on its job) or `closed` (neither: rejected, withdrawn or deleted), and
   each change gains a `status` record; `accepted` is an `awarded` event.
   When the hire read failed, a bid that is not pending is left as is.
3. Resend open intents that are still not on the market (one resend per
   pass; `duplicate_bid` closes as placed). A resent intent stays counted
   as open unless the market refuses it.
4. `open = pending bids + open intents`. List the board for each kind
   (with the query). For every job in order: skip if journaled, if a
   pending bid names it (`already_bid`), if it was skipped before for the
   same `updatedAt` (memo in `BidderState`, process memory only), on the
   filters of spec "Job choice" and "Job terms"; then, if `open >=
   max_open_bids`, skip `max_open_bids` (not memoised) before any further
   read or question; else read the job's attachments, run intake and the
   scope question (a passing answer is kept for the job's `updatedAt`),
   write the intent, place the bid, write the result, `open += 1`.
5. The whole bidder pass over every listing has one time budget
   (`board.pass_budget_secs`, default 90 s): the pass checks it before
   every read, question and bid, cuts a scope question at the time left,
   and leaves the rest for the next pass, so awards and the first-hour
   decline window of the next hire round are not held back.

### 4. Intake reuse

The job becomes a `Row` (`id` = `board:<jobId>`, title, description,
escrow = the bid amount, the job's attachments) and the pure checks of
`nearbee_core::intake` run on it with the listing's limits, protection and
price: daily loss (`cost::daily_loss` over the hires directory; an error
skips the job without memo), hard limits, cost admission, the `github_repo`
check, then the scope question through `Runner::decide` with a 60 s limit
when protection is on and a scope exists. A clear no is memoised; no
answer is taken, as at intake. At most 3 scope questions per pass, so a
crowded board never stalls the hire loop for long; the rest wait for the
next pass.

### 5. The proposal

`board::proposal(record, amount, sla)` renders, from the resolved listing
only:

```
I will do this job with the Nearbee agent <handle>: <scope>

How: a fixed playbook runs on the owner's machine (privacy tier <tier>).
Models: <list from direct, signed and script models, or "no model">.
Delivery: within <N hours> of the award, as Markdown with any files.
Data handling: the full data statement is appended to the deliverable.
```

At most 4000 characters (scope cut at a character boundary). No buyer
text, no model call, identical for every job of the listing at one
amount and SLA.

### 6. The bid journal

`<state_dir>/bids/<handle>/<jobId>.jsonl`, directories 0700, files 0600,
append with fsync (the hire journal's writer rules, a small separate
writer: the hire journal's records are hire-scoped). Records, one JSON
object per line with `seq`, `timestamp` and `type`:

- `intent` `{operation_id, amount, sla_seconds, proposal_sha256}`
- `result` `{operation_id, outcome: placed|refused, bid_id?, code?}`
- `status` `{status, bid_id?}` (`pending`, `accepted` or `closed`)

Recovery: an open intent (no result) is settled from the pending bids and
the assignments or resent
(step 3 above); a file that cannot be parsed counts as a bid on that job
(never bid again) and is reported every pass. No job text and no proposal
text is stored, only its digest. The journal's job id comes from the
board row and is a UUID; anything else is skipped (`bad_job_id`).

### 7. External calls

| Call | Timeout and retry | Idempotency |
| --- | --- | --- |
| `GET /v1/jobs/board` | the client's read budget (30 s per call, `Retry-After`) | read |
| `GET /v1/agents/me/bids?status=pending` | same | read |
| `GET /v1/jobs/{job_id}/attachments` | same | read |
| `POST /v1/jobs/{id}/bids` | the client's write rule (one resend on an unknown outcome); a throttled answer (`408`, `429`, a 4xx with `Retry-After`) is an unknown outcome sent once, and the intent stays open for a later pass | one bid per job on the market (`409 duplicate_bid`), plus the journal intent |
| `apb decide` (scope) | 60 s, killed at the limit | read |

### 8. Award and the hire path

No change to serve's hire round. A test in `crates/nearbee-core/tests/
serve.rs` feeds a row with `bidId` and a `standard` job through intake and
the run and checks the ceiling from the escrow.

### 9. Zero-byte output files

`plan_outputs` drops files of zero bytes and returns their paths; the
delivery journals one `output_skipped` record per path and generation
before the uploads, and the Markdown's Files list names only uploaded
files. The skip is not an error and does not need the operator.

### 10. Serve wiring

The serve loop keeps a `next_bid_at`; after a hire round, when the config
has a board block and the time is reached, it builds a market client per
board handle (the same token store) and runs the pass, then sets
`next_bid_at = round start + max(interval, poll)`. Bid events print at
info level as `bid <handle> <job>: <decision>`.

## Risks / Trade-offs

1. [The proposal is the same for every job of a listing; the buyer's bid
   list shows `proposalReuseCount`] -> accepted for the demo; a written
   proposal would need a model call or buyer text, both out of scope.
2. [The poster writes on the bid thread and nobody answers] -> the README
   says so; threads are out of scope.
3. [A bid placed and then the job edited] -> the bid stands; serve's
   intake runs again on the award row and can still decline `rejected`.
4. [`own_job`] -> the demo poster is a second account (Context 4).

## Migration Plan

None: no block, no change. Rollback is removing the block.
