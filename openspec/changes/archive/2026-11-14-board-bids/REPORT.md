# Report: board bids

Change `board-bids`, implemented 2026-11-14 in its own worktree off
`main`. Nothing is committed or pushed.

## What was measured (2026-11-14, read only)

All with the agent tokens of the owner's served listings, read from the
credentials store the way `serve` reads them; GET only, no market write
of any kind. Details and numbers are in `docs/market.md`, "Job board and
bids".

1. OpenAPI refreshed from `https://market.near.ai/openapi.json` and
   diffed against the 2026-11-09 copy: new board and bid fields (quote
   requests, `validUntil`, `revisedAt`), `PricingModel` gained `quote`,
   `OwnerAgentView` now requires `payable_with`. The existing contract
   test caught the last two; the wire types now follow them.
2. `GET /v1/jobs/board`: 40 public jobs; every row carries the poster
   (`creatorAccountId` and `creator.accountId`), `budgetToken` USDC 35 and
   USD 5, `jobKind` standard 36 and instant 4, `minBuilderVerification`
   none 39 and verified 1, 8 rows with no budget. No deadline and no
   attachments on standard job rows.
3. `GET /v1/agents/me/bids?status=all`: an instant hire already creates
   an `accepted` bid ("Direct hire at the listed price") and its
   assignment carries `bidId`. A board award is the same shape, so the
   hire path serves it unchanged.
4. `GET /v1/jobs/{id}/bids` as a non-creator: `401 unauthorized` (the
   OpenAPI says 404).
5. Worker skill 0.67.0 (was 0.55.0 in the vendored copy): one bid per
   job (`409 duplicate_bid`), one per builder (`409
   cluster_already_in_job`), `400 builder_verification_required`, `400
   usd_not_accepted`, `kind=standard` leaves competitions out.

## What was built

1. `board` block in `nearbee.yaml` (spec `nearbee-config`): interval,
   owner verification level, per-handle query, kinds, amount (default the
   listing price), SLA (default the listing SLA), open-bid cap (default
   3), poster allowlist (account UUIDs). Bounds checked at load;
   `config check` prints one line per board listing.
2. `Board` seam in `nearbee-core` and its `NearAgentMarket`
   implementation (board paging, my-bids paging, bid placement with
   `duplicate_bid` read as placed), contract-tested against the refreshed
   OpenAPI and wiremock-tested with schema-built bodies.
3. The bidder (`nearbee_core::board::pass`): settles the bid journal
   against the agent's bids (open intents closed or resent, status changes
   recorded, awards logged), filters board jobs, runs the listing's intake
   checks on the job (daily loss, hard limits, cost admission, the
   `github_repo` check, the scope question with at most three questions
   per pass and a memo), honours `max_open_bids`, and places one bid per
   job with a proposal generated from the listing record.
4. Bid journal `<state_dir>/bids/<handle>/<job id>.jsonl` (intent,
   result, status; fsync; 0600 files), no job text and no proposal text.
5. `nearbee serve` runs the pass after the hire round, once per board
   interval (never faster than the poll), and prints one `bid <handle>
   <job>: ...` line per decision. A `poster_not_allowed` skip names the
   poster's account id so the owner can copy it into the allowlist.
6. `nearbee bids [--json]` lists the bid journal.
7. Fix: a zero-byte output file is skipped, journaled once per
   generation as `output_skipped` (reason `empty`), and the rest is
   delivered.
8. Fix: `socat` added to `sandbox/Dockerfile` (image not rebuilt).
9. Docs: `docs/market.md`, `docs/architecture.md`, `docs/decisions.md`
   (D-028, D-011 annotated), `docs/privacy.md` (what the bid journal
   keeps), `README.md` step 10, `nearbee.example.yaml`.

## Gate

Run on the final tree, 2026-11-14, `CARGO_TARGET_DIR=~/.cache/nearbee-board-bids-target`:

```
cargo fmt --all --check                                  fmt exit=0
cargo clippy --workspace --all-targets -- -D warnings    clippy exit=0
cargo test --workspace                                   test exit=0
  42 test binaries: 892 passed, 0 failed, 5 ignored (the live and
  owner-only tests, ignored as on main)
openspec validate board-bids --strict                    Change 'board-bids' is valid
```

Of these, new or touched by this change: `nearbee-core` tests `board`
16, `serve` 223 (2 zero-byte and 1 award test added), lib 224 (config
board block); `nearbee-market` `contract` 11 (3 added), `client` 38 (5
added); `nearbee` `cli` 17 (2 added), `serve_full_hire` 7 (1 added),
`sandbox_image` 1.

## How the owner runs the board flow

1. Publish and store the listing as usual: `nearbee list <listing.yaml>`
   and `nearbee agent add <handle> --agent-id <uuid>` with the token on
   standard input. Serve must be able to serve the handle.
2. Add to the config `serve` runs with (for example
   `nearbee.local.yaml`):

   ```yaml
   board:
     interval_secs: 60
     owner_verification: none
     listings:
       astro-site:
         query: "astro"
         kinds: [standard]
         amount_usd: "2.00"
         sla_seconds: 86400
         max_open_bids: 3
         poster_allowlist: ["<account id of the posting account>"]
   ```

   `interval_secs: 60` keeps the demo short (300 is the default).
   `nearbee --config nearbee.local.yaml config check` prints the `board
   astro-site:` line.
3. Post the job from a **second market account** (not the owner of the
   agent): the market refuses a bid on its owner's own job (`own_job`).
   Web app: Post a job, a title and description that the listing's scope
   covers and that contain the query word, a budget in USDC at or above
   the amount (or none), standard kind.
4. The poster account id: leave `poster_allowlist` out for the first pass
   with a query only that job matches, or run serve once with any
   allowlist: the skip line `skipped (poster_not_allowed <uuid>)` prints
   the poster's id; put it in the allowlist and restart serve.
5. Restart `nearbee serve` (it reads the config at start). Within one
   interval it prints `bid astro-site <job>: matched` and `bid sent, USDC
   2.00, bid <id>`. `nearbee bids` shows it as `placed`, later `pending`.
6. In the web app, as the poster: open the job, Bids, award the
   astro-site bid (this funds the held payment). Serve prints `bid
   astro-site <job>: awarded` on its next pass, and the hire round takes
   the new assignment like an instant hire (intake, run, deliverable,
   submit). `nearbee hires` shows it.

## What is not done

1. No live bid was placed: the brief forbids market writes while
   building. The first real bid is the owner's demo run; `own_job` is
   read from the OpenAPI by its name, not measured.
2. Bid threads (the poster's messages on a bid before the award) are not
   read or answered; withdrawal, quote revision, quote requests,
   competitions and USD jobs are out of scope.
3. The proposal is the same for every job of a listing (the poster sees
   `proposalReuseCount`); a job-specific proposal would need buyer text or
   a model call, both excluded.
4. The skip memo lives in process memory: after a restart a job that was
   out of scope is asked about once more (at most three scope questions
   per pass).
5. The quality cycle of `docs/process.md` (OCR, cross-family review,
   audits, final review), the archive and the pull request are the
   integrator's. The sandbox image is not rebuilt.
6. Deviation from the process: no `brainstorm.md` (deadline, scope fixed
   by the owner; said in the proposal) and no `review.md` of the
   artifacts. Several tests were written in the same step as their code
   rather than strictly first.

## Review fixes (one round, 2026-11-14)

Each finding of the review was fixed with a test. Each new test was
checked to fail without its fix: the fix was reverted in the source, the
test was run and seen to fail, and the fix was put back (core:
`crates/nearbee-core/src/board.rs`; client: the `status=pending` query
and the throttle-aware bid write in `crates/nearbee-market/src/lib.rs`).

1. Open-bid cap after a resend. A resent open intent was uncounted on any
   result other than an error, so `max_open_bids` could be passed in the
   same pass. Now only a refusal uncounts it. Test
   `a_resent_intent_stays_counted_against_the_open_bid_cap`.
2. Throttled bids. `408` and `429` came back as refusals, were journaled
   `refused` and never retried. Now the bid write
   (`NearAgentMarket::write_bid`) reads `408`, `429` and any other 4xx
   with `Retry-After` as an unknown outcome and sends it once (no
   immediate resend), and the core keeps the intent open (also for a
   `Refused` 408 or 429 from any `Board`), so the next pass sends it
   again. Tests `a_throttled_bid_stays_open_and_is_sent_again` and
   `a_throttled_bid_is_an_unknown_outcome_sent_once` (429, 408, 400 with
   `Retry-After`). Other writes keep their old rule.
3. Cap before the scope question. The cap is checked right after the row
   filters, before the attachment read, intake and the scope question. A
   passing scope answer is kept in `BidderState` per job and `updatedAt`
   for the process lifetime. Tests `a_full_listing_asks_no_scope_question`
   (no question, no attachment read at the cap) and
   `a_passing_scope_answer_is_not_asked_again` (a read-only bid
   directory holds the job back twice: one question).
4. Pass time budget. New `board.pass_budget_secs` (15 to 300, default
   90). Serve computes one deadline for the whole pass over every
   listing; `board::pass` checks it before every read, question and bid,
   cuts a scope question at the time left, and stops with
   `time_budget_spent`; a listing not reached is logged as skipped. Tests
   `a_spent_time_budget_stops_the_pass`,
   `a_scope_question_waits_no_longer_than_the_budget` (a 30 s answer cut
   at 300 ms) and the config bounds. `config check` prints the budget.
5. Attachments at intake. `Board::job_attachments` reads
   `GET /v1/jobs/{job_id}/attachments` (`ListJobAttachmentsResponse`,
   checked against the OpenAPI: route, response component, required
   fields) for a job that passed the row filters and the cap; the files
   go on the row, so `hard_limits` checks their count and size. An
   unreadable list skips the job this pass with `attachments_unreadable
   (<status> <code>)`, not memoised. Tests
   `the_hard_limits_see_the_jobs_attachments` and
   `a_jobs_attachments_are_read_for_the_hard_limits`.
6. Bid list that grows. `Board::my_bids` (`status=all`) became
   `Board::pending_bids` (`status=pending`). The agent token has no read
   of one bid by id (`GET /v1/me/bids` is the owner's account route,
   `GET /v1/jobs/{id}/bids` the creator's), so awards come from the
   listing's assignment rows that the same loop iteration's hire round
   already read (`PassInputs::assigned_jobs`, the job ids): a journaled
   bid reads `pending`, `accepted` (an assignment on its job, logged as
   awarded) or `closed` (neither: rejected, withdrawn or deleted); when
   the hire read failed, a bid that is not pending is left as is. An open
   intent whose job has an assignment closes as placed without a resend.
   The journal's `status` record now has an optional `bid_id` (old lines
   still read). Tests
   `pending_bids_read_only_the_pending_status_until_the_total`,
   `a_bid_off_the_pending_list_settles_from_the_assignments`,
   `an_open_intent_awarded_meanwhile_is_not_resent`, and the serve test
   now mocks `status=pending` and the attachment route.
7. `requiresVerifiable`. Read from the job (required in `Job`) and
   skipped with `verifiable_required`. Row filter case and a contract
   test that the field parses and is required.

Specs (`board-bidding`, `market-client`, `nearbee-config`), `design.md`,
`docs/architecture.md`, `docs/market.md`, `nearbee.example.yaml` and
`tasks.md` (section 8) follow the fixes.

Gate after the fixes (2026-11-14):

| Step | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | 903 passed, 0 failed, 5 ignored (42 test binaries) |
| `cargo nextest run --workspace` | 903 passed, 5 skipped |
| `cargo run -p nearbee --locked -- replay` | 5 of 5 scenarios ok |
| `openspec validate board-bids --strict` | valid |
| `openspec validate --all --strict` | 22 passed, 0 failed |

Board tests: 25 in `crates/nearbee-core/tests/board.rs` (9 new); market
client 40 and contract 11 tests pass.

Not done in this round:

1. An award whose funding fails leaves a cancelled assignment row on the
   job; if that bid later ends rejected while the row is still listed,
   the journal reads `accepted` rather than `closed`. The hire round is
   not affected (it judges the row itself); only the `nearbee bids` state
   word can be off. Filtering cancelled rows needs the wire status of
   such a row, which was not measured.
2. A bid placed by the agent outside Nearbee that is no longer pending is
   not seen as `already_bid`; a bid on such a job is answered `409
   duplicate_bid` and closes as placed.
