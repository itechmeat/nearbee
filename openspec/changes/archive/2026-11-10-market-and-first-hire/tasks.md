# Tasks: market and first hire

Implementer rules: `docs/process.md`. Stage values: change
`market-and-first-hire`, so `CARGO_TARGET_DIR=$HOME/.cache/nearbee-market-and-first-hire-target`.
Before a group, read its part of `design.md` and the spec it names. Every
hire in this stage is the owner's own test; no third-party data.
Tasks marked **owner** need something only the owner gives: stop there, leave
the box open and report what is needed; continue with the next group. O-1 and O-2 (the first variant `repo-audit-fast` as a draft
agent with its token) were provided before this change.

## 1. Gate

- [x] 1.1 `deny.toml` and `cargo deny check` (advisories, licenses, bans, sources) green locally; CI installs a pinned `cargo-deny` and runs it; verify with the CI file parse and a local run.

## 2. Market types and client (spec `market-client`)

- [x] 2.1 `Market` trait, row and request types, classification in `nearbee-core::market`, and a `DeliveryId` type for the v1.1 webhook `X-Market-Delivery` header (type and parse only, no webhook code); unit tests for every classification scenario and the header parse.
- [x] 2.2 `NearAgentMarket` transport and error classes (design 2); wiremock tests: 3xx on a write is unknown, 401 is unauthorized and not retried, capability link and token redacted, reads retried with `Retry-After`, client built with retries disabled.
- [x] 2.3 Paging over `status=all`; tests for two pages, an empty list.
- [x] 2.4 Write methods with their retry rules; tests for submit resent identically, a write still unknown after exactly two requests, decline repeat answered `invalid_state_transition` counted as done only when a fresh read shows the row cancelled (refused otherwise), a read retried after 503 with `Retry-After`, progress after `409` waits `retry_after`, messages carry `clientRequestId`.
- [x] 2.5 File upload trio; test with wiremock (reserve, PUT exact bytes without extra headers, complete).

## 3. Credentials (spec `agent-credentials`)

- [x] 3.1 Credentials file read and write in `nearbee-core::credentials`; tests for mode refusal, atomic replace keeping other handles, token format.
- [x] 3.2 `nearbee agent add <handle>` (stdin token, public agent lookup, assignments probe, masked output); CLI tests against wiremock for unknown handle, refused token, success.
- [x] 3.3 Token-like values refused in config and listing files; tests.
- [ ] 3.4 Capture fixtures that need no hire (design 3): add the real `repo-audit-fast` token with `grep '^NEARBEE_TOKEN_REPO_AUDIT_FAST=' .env | cut -d= -f2- | nearbee agent add repo-audit-fast --agent-id <uuid>` (the agent id copied from the agent's settings page in the market web app: a draft agent answers 404 to every lookup its token can make, measured 2026-11-10; value never printed), then capture the public agent record, an empty or current `assignments?status=all`, pricing plans list, the file trio with a delete of the unbound file; replay tests for each. Then remove the `NEARBEE_TOKEN_REPO_AUDIT_FAST` line from `.env` and from `.env.example`'s documentation, and say so in the report.

## 4. Journal writer (spec `hire-journal`)

- [x] 4.1 Append with consecutive `seq`, fsync of file and directory before returning, reopen continues the sequence, a truncated tail refuses writing; tests with temp dirs, including a test that the record is on disk before a fake external call runs.

## 5. Listing and publish (spec `listing-publish`)

- [x] 5.1 Listing types and validation in `nearbee-core::listing`; tests for every scenario of the spec.
- [x] 5.2 `playbooks/first-hire/` (design 5) on `nearbee-own`, writing a manifest; `apb validate` clean.
- [x] 5.3 Materialisation and `install_playbook` in `nearbee-apb` (bundle import, preview must say unchanged, approve exactly that bundle, read the installed current version back and refuse a source or installed version without `trial_candidates: off`); tests with the fake `apb` pattern of S1; `scripts/apb-instance.sh install` reuses it or stays consistent with it.
- [ ] 5.4 `nearbee list <listing.yaml>`: materialise, patch agent, pricing plan only when price or SLA changed, make default, never `listing_status`, fail naming the cap on a pricing-plan `409`; wiremock tests for each scenario; then publish `playbooks/first-hire/listing.repo-audit-fast.yaml` for real (draft stays draft) and capture the patch and pricing-plan answers as fixtures.

## 6. Serve (spec `hire-serving`)

- [x] 6.1 Serve state machine in `nearbee-core::serve` over fake `Market` and `Runner`: polling filter, taking a hire, owner-started rows, delivery, invalid manifest declines `failed`, timeout declines `failed`, refused submit declines `failed` with the machine code, unknown submit outcome reported with the intent kept open, rework and redo logged and left, cancelled rows stop local work, unauthorized stops one agent only; tests for each.
- [x] 6.2 Restart resume from the journal: run intent resolved by `execution_id` lookup, incomplete lookup reports instead of starting, submit intent completed by an identical resend; tests.
- [x] 6.3 `serve --reject <handle>`; test.
- [x] 6.4 `nearbee serve` command wiring the real client and runner; a test against wiremock market plus wiremock apb for one full hire.

## 7. First end-to-end hire (owner)

- [ ] 7.1 **owner**: the owner hires the draft `repo-audit-fast` from the market web app with a short synthetic brief while `nearbee serve` runs. Verify: the hire is started, the run succeeds, the Markdown is submitted, the owner sees it on the job page. Capture the hire-time fixtures (row, start, progress, messages, submit). Record task id, timestamps and the submitted digest in `docs/demo-log.md` (create it).
- [ ] 7.2 **owner**: a second owner hire with `serve --reject repo-audit-fast`; verify it is declined `rejected`; capture the decline answer and the refunded row.

## 8. x402 caller (spec `x402-caller`)

- [x] 8.1 `examples/x402-hire` (design 7) with `near-api-js` v7 (newest stable checked); `--dry-run` path; unit test of the payment payload and the local verification with a throwaway key generated in the test (no network).
- [ ] 8.2 **owner** (O-5: `NEAR_CALLER_ACCOUNT_ID`, `NEAR_CALLER_PRIVATE_KEY` in `examples/x402-hire/.env`): `--dry-run` against the live agent card of a live market agent of the owner's choosing or `repo-audit-fast` once live; the local verification passes; record the result in `docs/market.md`.
- [ ] 8.3 **owner** (O-6: the owner sets `repo-audit-fast` live at the lowest price for the test): run `serve --reject repo-audit-fast`, run the example for real, verify the hire is declined `rejected`, `GetTask` ends refunded and the caller's USDC balance is unchanged; record in `docs/demo-log.md` and `docs/market.md`; tell the owner they may set the agent back to draft.

## 9. Carried from S1 (owner)

- [ ] 9.1 **owner** (O-3 top-up and the go-ahead for `scripts/apb-instance.sh allow-billed`): run `live_smoke_qwen` and `live_smoke_deepseek` (`NEARBEE_LIVE=1`), record the results as S1 task 7.3 described in `docs/apb.md` and `docs/near-ai-cloud.md`.

## 10. Docs and gate

- [x] 10.1 `docs/market.md` (every live fact the fixtures showed, differences from the vendored docs), `docs/architecture.md` (serve flow as built, credentials, journal writer in S2), `README.md` "Try it" (`agent add`, `list`, `serve`), `docs/owner-checklist.md` statuses.
- [x] 10.2 Full native gate green: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`, `openspec validate --all --strict`, and `node --test` in `examples/x402-hire`; paste the tails into the report.

## Carried to S3

Open at merge (2026-11-10) and moved to the start of S3 (`docs/stages.md`):
3.4, the live part of 5.4, 7.1, 7.2, 8.2, 8.3, 9.1.

## Workflow follow-up

- Owner-gated tasks still open at the end are reported; the integrator asks the owner or moves them with the owner's word, as in S1.
- The integrator runs the quality cycle, archives the change and opens the pull request.
