# Tasks

## 1. Facts

- [x] 1.1 Refresh `docs/reference/market-openapi.json` from the live API, diff it and record the board, bid and assignment facts with the read-only measurements of 2026-11-14 in `docs/market.md`; verify `cargo test -p nearbee-market --test contract` still passes on the new file

## 2. Market client

- [x] 2.1 Wire types `ListBoardJobsResponse`, `JobView`, `MyBidsResponse`, `Bid`, `PlaceBidRequest` and their mapping to the core `BoardJob`, `MyBid`; contract tests deserialize schema-built instances and check the bid request body against `PlaceBidRequest`; verify `cargo test -p nearbee-market --test contract`
- [x] 2.2 `Board` trait in `nearbee-core` and its `NearAgentMarket` implementation (board paging, my bids paging, bid placement with `duplicate_bid` as placed); wiremock tests built from the OpenAPI fixture; verify `cargo test -p nearbee-market`

## 3. Config

- [x] 3.1 `board` block in `nearbee-core::config` with its bounds and defaults, refused values named; `config check` lines; `nearbee.example.yaml` documents it commented out; verify `cargo test -p nearbee-core config` and `cargo test -p nearbee --test cli`

## 4. Bidder

- [x] 4.1 Bid journal (`<state_dir>/bids/<handle>/<job>.jsonl`, intent, result, status; fsync; unreadable file counts as bid); unit tests for append, read and recovery; verify `cargo test -p nearbee-core board`
- [x] 4.2 Job filters, intake reuse with the scope memo, generated proposal; unit tests for each skip reason, the no-buyer-text proposal and the scope memo; verify `cargo test -p nearbee-core board`
- [x] 4.3 The pass: settle intents from my bids, status changes and award events, open-bid cap, refusal recorded; tests with a fake `Board` for the restart, refused, already-bid, cap and award scenarios; verify `cargo test -p nearbee-core board`

## 5. Serve and CLI

- [x] 5.1 Serve runs the pass on its own interval after the hire round and logs each decision; `nearbee bids [--json]`; CLI tests for `bids`; verify `cargo test -p nearbee`
- [x] 5.2 Award path test: a row with `bidId` and a `standard` job is served like an instant hire with the ceiling from its escrow; verify `cargo test -p nearbee-core --test serve board`

## 6. Fixes

- [x] 6.1 Zero-byte output files skipped and journaled (`output_skipped`); tests for an empty file beside another and for an empty file alone; verify `cargo test -p nearbee-core --test serve empty`
- [x] 6.2 `socat` in `sandbox/Dockerfile` (no image rebuild here); verify `cargo test -p nearbee --test sandbox_image`

## 7. Docs and gate

- [x] 7.1 `docs/architecture.md` (bidder, bid journal), `README.md` (board flow, do X see Y), `docs/decisions.md` D-028, `docs/privacy.md` (what the bid journal keeps); verify by reading them against the specs
- [x] 7.2 Full native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate board-bids --strict`; counts in `REPORT.md`

## 8. Review fixes (one round)

- [x] 8.1 A resent intent stays counted against `max_open_bids` unless refused; test `a_resent_intent_stays_counted_against_the_open_bid_cap`; verify `cargo test -p nearbee-core --test board`
- [x] 8.2 A throttled bid (`408`, `429`, `Retry-After`) keeps its intent open and is sent again on a later pass; tests `a_throttled_bid_stays_open_and_is_sent_again` (core) and `a_throttled_bid_is_an_unknown_outcome_sent_once` (client); verify `cargo test -p nearbee-core --test board` and `cargo test -p nearbee-market --test client`
- [x] 8.3 The open-bid cap is checked before the attachment read, intake and the scope question; a passing scope answer is kept per job version; tests `a_full_listing_asks_no_scope_question` and `a_passing_scope_answer_is_not_asked_again`
- [x] 8.4 One time budget for the whole bidder pass (`board.pass_budget_secs`, 15 to 300, default 90); tests `a_spent_time_budget_stops_the_pass`, `a_scope_question_waits_no_longer_than_the_budget` and the config bounds
- [x] 8.5 The job's attachments (`GET /v1/jobs/{job_id}/attachments`) reach the hard limits; unreadable skips with `attachments_unreadable`; tests `the_hard_limits_see_the_jobs_attachments` and `a_jobs_attachments_are_read_for_the_hard_limits`, contract route and required fields
- [x] 8.6 Only pending bids are read; awards come from the listing's assignment job ids; tests `pending_bids_read_only_the_pending_status_until_the_total`, `a_bid_off_the_pending_list_settles_from_the_assignments`, `an_open_intent_awarded_meanwhile_is_not_resent`
- [x] 8.7 A job with `requiresVerifiable` is skipped with `verifiable_required`; row filter case and contract parse of the required field

## Workflow follow-up

- The integrator runs the quality cycle of `docs/process.md`, archives the change and opens the pull request.
