# Tasks: durable hire machine

Implementer rules: `docs/process.md`. Stage values: change
`durable-hire-machine`, so `CARGO_TARGET_DIR=$HOME/.cache/nearbee-durable-hire-machine-target`.
Before a group, read its part of `design.md` and the spec it names. Every
hire in this stage is the owner's own test (D-023). Tasks marked **owner**
need something only the owner gives: stop there, leave the box open, report
what is needed, continue with the next group. Every group ends green on the
tests of the crates it touched plus `cargo check --workspace --all-targets`,
and lands the docs its work changes.

Group 0 (except 0.5) is already in the tree, uncommitted: keep those
edits. The only market writes in this change are the file trio and its
delete in task 3.6; no spend, no `listing_status`, nothing in the owner's
name. Report to `.reviews/durable-hire-machine-implementer.md`; when that file already exists, read its "Next session handover" section first, continue from it and append to the report instead of rewriting it.

Seam rule for every group: market wire types are checked against
`docs/reference/market-openapi.json` (contract test, schema-built mock
bodies from `crates/nearbee-market-fixture`, never bodies written to match
the code); apb CLI calls are tested against fake `apb` scripts that print
the real wording recorded in `docs/apb.md` or read from apb's source at
`v1.0.0-beta.6` (`git -C ~/projects/tools/agentic-playbooks show v1.0.0-beta.6:<path>`,
read only).

## 0. Live fixes (2026-11-10)

- [x] 0.1 Numeric derived playbook version `1.0.<n>`; test `the_derived_version_is_numeric_semver_and_stable` passes.
- [x] 0.2 `extract_digest` reads apb's "--digest sha256:" approval prompt (exit 2); the fake apb in `install.rs` tests prints that wording.
- [x] 0.3 `apb export <id> --out <file>`; both fake apbs updated; the publish test checks a numeric patch version.
- [x] 0.4 SLA margin `min(10 min, window / 5)`; test `a_short_sla_keeps_most_of_its_window` passes.
- [x] 0.5 Derive the version from the whole bundle (playbook text, every script's bytes and executable flag, spec `listing-publish`, Materialisation); test: a script-only change gives a new version, an unchanged bundle the same one.

## 1. Journal (spec `hire-journal`)

- [x] 1.1 `generation` on every record (absent reads as 1) and the new event types and intent kinds of design 2 with round-trip tests; a terminal `run_observed` carries `cost_usd`, `cost_complete`, `budget_usd`.
- [x] 1.2 Private snapshots (`gen-<n>/`, 0700 and 0600, temp plus fsync plus rename) written before their intent; tests: snapshot on disk before a fake external call; a mismatching digest sets attention and sends nothing.
- [x] 1.3 `read_journal` maps only `NotFound` to no journal; test with an unreadable journal file.
- [x] 1.4 S2 compatibility (design, Migration Plan): S2-shaped journals (decided, running, terminal undecided, open `run_start`, open `market_submit`) read as generation 1; running is followed, terminal undecided is validated and snapshotted before its submit intent, the open intents get `attention s2_pending` and nothing is sent; tests with one fixture per shape.

## 2. Runner (spec `apb-runner`, spec `nearbee-config`)

- [x] 2.1 `Option<Budget>` and `continued_from` in `StartRequest`; `nearbee-apb` omits the `budget` key when absent and sends `continued_from`; serve keeps passing its current budget until task 6.3; wiremock tests for both bodies.
- [x] 2.2 `Usage` with `cost_by_basis` and the counters `attempts`, `finished_attempts`, `cost_attempts`, and `budget_paused` in `RunDetail`, parsed from the recorded fixture shapes in `crates/nearbee-apb/tests/fixtures/apb/` (add fixtures with `budget.paused` and with a partial cost, shaped as apb's `RunUsage` at the tag); tests: missing cost stays `None`, partial cost is incomplete.
- [x] 2.3 Config key `apb.config_dir` (default, `~` expansion, refusal of `~/.config/apb`) with config tests; `Runner::abort` through `apb stop <run>` with that `APB_CONFIG_DIR`, run from the workspace; fake-apb tests: exit 0, non-zero, timeout 30 s.
- [x] 2.4 `Runner::decide` through `apb decide is <q> --criteria <c> --no-run` (argv, no shell, `APB_RUN_ID` and `APB_NODE_ID` removed, 60 s timeout); fake-apb tests: answered yes `p` 0.9, no `p` 0.05, no `p` 0.35, `{"answered":false,"refused":"no_provider"}` (the exact line measured on Nearbee's instance), a provider `error`, a timeout; a brief with `$(...)` and quotes arrives as one argument.
- [x] 2.5 Docs: `docs/apb.md` (abort through `apb stop`, decide answers and refusals, `version: null` on the first detail, `continued_from` facts, usage counters, no run deletion at the tag).

## 3. Market client (spec `market-client`)

- [x] 3.1 Row fields of the spec in the core `Row` and the wire types; contract test extended for `WorkerJobBrief.attachments`, `AttachmentBrief`, `LatestMessageBrief` and `JobAssignment.finalizedAt`, `escrowAmount`.
- [x] 3.2 Read time budget of 30 s per call; wiremock test: `Retry-After: 120` returns a retryable read at once.
- [x] 3.3 `Market::messages` (Thread read) against `ListMessagesResponse` with schema-built bodies; tests: the full body of a truncated latest message found by `createdAt`, paging stops at the given time.
- [x] 3.3a `Market::download` (stream, cap, no redirect, bearer to the origin only); wiremock tests: 200 streamed, 302 not followed, 503 with `Retry-After`, 403, a body longer than the cap.
- [x] 3.4 `progress` returns the `409 progress_report_too_soon` delay at once (MODIFIED Decline and progress repeats); serve's existing progress call adapts in the same task; tests.
- [x] 3.5 Fix `crates/nearbee-market/examples/capture.rs` (design 15: agent id for pricing plans, `ReserveFileRequest {filename, byteSize, link}`, status checked before writing, delete of the unbound file); `cargo build -p nearbee-market --examples`.
- [x] 3.6 Live capture (the token is already in the credentials store, no hire, no spend): run the capture example for `repo-audit-fast`, scrub the fixtures (synthetic text where a body holds buyer text), add replay tests for each captured answer (assignments with the S2 test hires, pricing plans, the file trio and its delete). Then remove the `NEARBEE_TOKEN_REPO_AUDIT_FAST` line from `.env` (by line, never printing the value) and its mention from `.env.example`; say so in the report. Closes S2 task 3.4.
- [x] 3.7 Docs: `docs/market.md` (403 on agent writes with the agent token, test run with `escrowAmount` 0 and a 600 s SLA, live fixture differences from the OpenAPI, the read time budget).

## 4. Listing and `nearbee list` (spec `listing-publish`)

- [x] 4.1 Listing file keys `scope`, `protection`, `limits` with validation; tests: unknown limit refused; protected `0.26` refused and `0.264` loads (the formula `price * 0.95 - 0.25 > 0`, not a cent rule).
- [x] 4.2 `nearbee list` without market writes: install, record (with the new fields), read pricing plans, print the web-app settings table with bio and the `unverified` SLA (design 13); wiremock tests: no plan, matching plan, price mismatch, `null` plan SLA, and a request log with only GETs.
- [x] 4.3 Docs: `README.md` "Try it" (`list` installs and prints what to set in the web app), `docs/architecture.md` flow 1, `docs/decisions.md` D-024 (no market writes from the agent token; the client methods stay for the deferred owner sign-in).

## 5. Serve: rounds, lifecycle, recovery (spec `hire-serving`)

- [x] 5.1 A test driver for multi-round serve tests with a controllable clock over the fake `Market` and `Runner` in `crates/nearbee-core/tests/serve.rs`; port the S2 tests that expected a delivery from one `poll_agent` call to the driver, keeping their assertions.
- [x] 5.2 Round structure (design 1): reconcile, short steps with every wall clock, at most one long step, admit; `observe_once` replaces `run_to_completion`; tests: another hire's cancellation handled while one run is active; a slow download of hire A is cut off at hire B's deadline and retried later; progress too soon records a due time and the round does not sleep.
- [x] 5.3 Version check with a pending first observation; tests: `null` then the recorded version is followed; terminal with `null` declines `failed`; `null` for more than 60 s declines `failed`.
- [x] 5.4 Pending submit and decline settled against the row first, then the lifecycle transitions of design 3; tests: submit answer lost then `submitted`, then rework, then a dispute redo, then `accepted` (no resend in each; rework and redo open a generation); identical resend only while the row awaits it; accepted with `finalizedAt`; cancelled while running (abort first); disputed freezes; dispute resolved aborts and closes; silence past `autoAcceptAt` records nothing; a non-`in_progress` row without a journal writes nothing.
- [x] 5.5 Abort (design 4), local work after the market closes, and the one slot (design 5); tests: unconfirmed abort retried and holding the slot; accepted while a rework run is active keeps the hire in reconciliation until terminal; a second hire waits after intake and starts after the first ends; a rework arriving during another run waits for the slot; an unreadable journal holds the slot; a waiting first-time hire past its deadline is declined `rejected`, a waiting rework `failed`.
- [x] 5.6 Unresolved starts and stray runs (design 6); tests: one match adopted; two matches attention; complete empty lookups start nothing, set `start_unresolved` after 120 s, decline `failed` at the deadline and keep the slot; a start landing after the deadline (and after the hire closed) is found by `hire_id` and aborted, its cost recorded, never submitted.
- [x] 5.7 Lock file (design 12) used by `serve` and `cleanup`; tests: a second lock attempt fails with the exit-4 error; a child process does not inherit the lock.
- [x] 5.8 Docs: `docs/architecture.md` (serve flow and recovery table as built, generations, the one slot, the lock, the operator procedures for an unresolved start and for `s2_pending`), `docs/decisions.md` D-025 (one active run until per-hire git worktrees).

## 6. Intake and cost (spec `hire-intake`)

- [x] 6.1 `nearbee-core::intake`: order of checks, hard limits, decline texts of design 7, first-hour flag; tests for every scenario of the spec, the brief never in a journal record.
- [x] 6.2 Scope check over `Runner::decide` with the `scope_check` intent: clear no declines, uncertain taken, unanswered skipped with its reason and one operator notice per process, an interrupted check skipped and never asked again; tests.
- [x] 6.3 Ceiling from `escrowAmount` or the listing price for a zero escrow; per-run `budget.usd = ceiling - spent`; spent from terminal `run_observed` costs with an incomplete or missing cost counting as the run's budget; `protection: off` omits the budget; a budget pause aborts and declines `failed`; remove the fixed USD 5 budget from `crates/nearbee/src/lib.rs`; tests for each, including a crash between the terminal observation and the delivery (the cost is already in the terminal record).
- [x] 6.4 Daily loss limit by UTC day (config `limits.daily_loss_usd`, spec `nearbee-config`, with config tests); tests: limit reached declines `rejected` with one notice, the next UTC day starts from zero, a stray run's cost counts.
- [x] 6.5 Docs: `docs/pricing.md` (ceiling from the award, cost basis and completeness, protection off, daily loss as built), `nearbee.example.yaml` (`limits.daily_loss_usd`, `apb.config_dir`).

## 7. Attachments (spec `hire-attachments`)

- [x] 7.1 `nearbee-core::attachments`: plan, origin check, size and hash verification, `.partial` plus rename, safe names, hire-wide indices, dedup by attachment id, the `inputs` param; tests for every scenario of the spec (foreign host, relative path, hash mismatch, overlong stream, 503 due time, 403 declines, unsafe name, same name in two reworks, already stored, crash mid-download, two files param).
- [x] 7.2 One worktree per hire (design 11) with S2 hires keeping their journaled paths; test.

## 8. Rework and redo (spec `hire-serving`)

- [x] 8.1 Generations keyed by the row (design 2 and 3); tests: three polls of one rework row open one generation and start one run; a second rework opens generation 3.
- [x] 8.2 Continued run: `continued_from` the latest run, `brief`, `feedback` (requirement Rework feedback), message attachments added to `inputs`, `out/` moved to the previous generation's snapshot; tests: rework params, truncated feedback read in full from the thread, latest message not the buyer's declines `failed` with `feedback_unavailable`, redo with empty `feedback`, stale manifest declines `failed`, used-up budget declines `failed`, rework after a republish declines `failed`.
- [x] 8.3 `playbooks/first-hire/repo-audit-fast.yaml`: declare `feedback` and `inputs` as untrusted params and use them in the prompt (revise the earlier note when `feedback` is set; read the files listed in `inputs`); `apb validate` clean; republish with `nearbee list` before task 10.1 (the hires of 10.1 and 10.2 must run the same version).

## 9. `nearbee hires` and `nearbee cleanup` (spec `hire-retention`)

- [x] 9.1 `nearbee-core::retention` summaries and `nearbee hires` (`--json`); CLI tests: attention code shown, a broken journal does not hide the others, no brief text in the output.
- [x] 9.2 `nearbee cleanup` and `--dry-run` with eligibility and the inventory (config `retention.after_acceptance_days` with config tests); tests: recently accepted left, declined at intake deleted, cancelled after a delivery deleted after the retention days, disputed never deleted, a worktree outside `work/` refused, partial cleanup recorded and retried, dry run changes nothing.
- [x] 9.3 Docs: `docs/privacy.md` (deletion inventory as built, the not covered items), `README.md` (`hires`, `cleanup`).

## 10. Live checks

- [ ] 10.1 **owner** (an owner "Test run" hire of `repo-audit-fast`, no spend): `nearbee serve` from this tree; kill it with `pkill -x nearbee` while the run is active, start it again; verify one apb run, one submit, the journal shows the resume. Record in `docs/demo-log.md`.
- [ ] 10.2 **owner**: the owner requests changes on the delivered hire of 10.1 with a short message (and one small attached file); verify generation 2 runs as a continued run with the message as `feedback` and the file in `inputs`, and that the delivered note reflects the request. Record in `docs/demo-log.md`.
- [ ] 10.3 After 10.1 and 10.2: `nearbee hires` shows both hires correctly; `nearbee cleanup --dry-run` lists the declined test hire of 2026-11-10 as eligible; paste both outputs into the report.

## 11. Carried owner tasks from S2

- [ ] 11.1 **owner** (O-5, O-6): S2 tasks 8.2 and 8.3, the x402 dry run and the rejected paid call, as written in `openspec/changes/archive/2026-11-10-market-and-first-hire/tasks.md`.
- [ ] 11.2 **owner** (O-3 top-up and the go-ahead for `scripts/apb-instance.sh allow-billed`): S2 task 9.1, the billed Cloud smokes.

## 12. Integration

- [x] 12.1 Consistency pass over the docs the groups changed plus `docs/stages.md` (S3 as built, carried items) and `docs/owner-checklist.md` statuses.
- [x] 12.2 Full native gate green: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`, `openspec validate --all --strict`, `node --test` in `examples/x402-hire`; paste the tails into the report.

## Workflow follow-up

- Owner-gated tasks still open at the end are reported; the integrator asks the owner at that point.
- The integrator runs the quality cycle, archives the change and opens the pull request.
