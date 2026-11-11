# Stages

The plan of `docs/plan.md` grouped into six stages. **Each stage is one
OpenSpec change and one pull request**, built and checked with the cycle of
`docs/process.md`. Stages run in order: one pull request open at a time; the
next stage's OpenSpec artifacts (brainstorm to review) are written while the
current stage is implemented, so implementation of the next stage starts as
soon as the current one merges.

Each entry says what the stage delivers, which plan items it covers, which
owner items it needs (`docs/owner-checklist.md`), what proves it done, and what
it leaves out. The OpenSpec change name is fixed here so the record stays
traceable.

| # | Change name | Plan items | Target day | Depends on |
| --- | --- | --- | --- | --- |
| S1 | `foundation-and-runner` | 3, 4, 5 | 2026-11-10 | none |
| S2 | `market-and-first-hire` | 6, 7 | 2026-11-10 to 11 | S1 |
| S3 | `durable-hire-machine` | 8, 9, 10, 11, 12 | 2026-11-11 | S2 |
| S4 | `repo-audit-and-tiers` | 13, 14, 15 | 2026-11-12 | S3 |
| S5 | `doc-digest-and-paid-demo` | 16, 17 | 2026-11-13 | S4 |
| S6 | `judges-path-and-submission` | 18, 19, 20 | 2026-11-14 | S5 |

## S1. `foundation-and-runner`

Delivers:

1. The cargo workspace (`crates/nearbee-core`, `nearbee-market`,
   `nearbee-apb`, `nearbee`), config loading from file plus environment,
   `.env.example` with placeholders, the hire journal format (types and
   on-disk layout only), the `Market` and `Runner` traits, a CI workflow
   running the native gate.
2. The **apb runner**: `nearbee-apb` against a local `apb dashboard`: list
   playbooks, estimate, start with `hire_id` as a `search: true` param, poll
   `RunDetail`, read `outputs.files`, find a run by `hire_id`, read and
   validate the `out/result.json` manifest (containment, sizes, sha256, the
   256 KiB limit). Contract tests against recorded responses of a real apb.
3. The **Cloud path**: apb profile templates `own`, `near`, `near-vision`;
   one trivial playbook run on `Qwen/Qwen3.8-27B` and on
   `deepseek-ai/DeepSeek-V4.1-Flash-1017` through the executor path apb
   supports at that moment (OpenCode custom provider today, apb's native NEAR
   support if it has landed); results recorded in `docs/apb.md` and
   `docs/near-ai-cloud.md` (open question 11 partly, 12).

Owner items: O-3 (NEAR AI Cloud key, credits topped up before the live Cloud
runs). O-1 and O-2 were provided on 2026-11-09 ahead of S2.

Done when: native gate green; the runner's contract tests pass offline; `nearbee runs find
--hire-id` (or the equivalent library test) finds a REST-started run.

Out of scope: market code, intake, journal recovery logic.

## S2. `market-and-first-hire`

Delivers:

0. Carried from S1 (owner, 2026-11-09): the two billed smoke runs on
   `Qwen/Qwen3.8-27B` and `deepseek-ai/DeepSeek-V4.1-Flash-1017` through
   `ApbRunner` once the NEAR AI Cloud balance is topped up and the owner
   allows billed launches on Nearbee's apb instance
   (`scripts/apb-instance.sh allow-billed`); results into `docs/apb.md` and
   `docs/near-ai-cloud.md`. The tests are ready:
   `NEARBEE_LIVE=1 cargo test -p nearbee-apb --test live -- --ignored live_smoke_qwen`
   and `live_smoke_deepseek`.
1. `nearbee-market`: assignments in all statuses with paging, start, messages
   with `clientRequestId`, progress, file upload, submit, decline, agent patch,
   pricing plans; recorded real responses as fixtures; `X-Market-Delivery`
   dedup kept for v1.1 webhooks only as a type.
2. Agent tokens are owned by Nearbee, not by `.env`: `nearbee agent add
   <handle>` takes an `aat_` token once (stdin, never an argument) and stores
   it in `~/.config/nearbee/credentials` (mode 0600, outside the repository);
   config and listings name agents by handle only; the token of
   `repo-audit-fast` moves there from `.env` (D-022).
3. `nearbee list` publishes one draft listing from a materialised playbook
   whose installed current version carries `trial_candidates: off` (apb would
   otherwise run a candidate version, `docs/apb.md`): materialisation sets
   it, and publishing refuses a source playbook that sets another value or
   an installed version that does not read back with it;
   `nearbee serve` polls, takes a hire, runs a trivial playbook, submits the
   manifest's Markdown. **First end-to-end hire**: the owner hires the draft
   from the web.
4. `examples/x402-hire`: builds and signs a NEP-366 delegate for a 402
   response and proves key type and relayer acceptance without spending
   (open question 13); falls back to `near-api-js` if the Rust path is
   awkward (D-003). No-spend proof: the demo caller account (O-5) pays the
   owner's agent while the owner has it live for the test (O-6; draft agents
   take no x402 calls, `docs/market.md`), `serve` declines the hire with
   `rejected`, and the hold is released (nothing is charged on a rejected hire, `docs/market.md`).
5. `cargo deny check advisories` (and licenses) in the native gate and CI
   (security-audit recommendation from S1).

All hires in S2 are the owner's own tests (the owner's web hire of the draft,
the demo caller account), so no third-party data runs and apb's
`external_data` flag is not needed yet (D-023).

Owner items: O-1, O-2 (first variant), O-5 (asked at the start of S2; no
spend, signature only), O-3 top-up and the billed-launch go-ahead for item 0.

Done when: native gate green; the end-to-end hire is recorded (task id,
timestamps) in `docs/demo-log.md`; the signature check result is in
`docs/market.md`.

Out of scope: recovery after a crash, intake decisions, attachments, paid
hires (a hire that settles money; item 0's billed Cloud runs are model spend,
not hires).

## S3. `durable-hire-machine`

Carried from S2 (owner items open when S2 merged, 2026-11-10; tasks of
`openspec/changes/archive/*-market-and-first-hire/tasks.md`): 3.4 recorded
market fixtures and the move of the `repo-audit-fast` token from `.env` into
the credentials store (needs the draft's agent id from the owner, O-2:
drafts are invisible to every lookup the agent token can make); the real
draft publish of 5.4; the owner's web hires 7.1 and 7.2 (first end-to-end
hire into `docs/demo-log.md`); the x402 proof 8.2 and 8.3 (O-5, O-6); the
billed Cloud smokes 9.1 (O-3). They run first in S3, before its own work, and
the recorded fixtures replace the schema-shaped mock bodies where they
differ.

Delivers: the journal with intents before every external write and the
recovery table; the lock file; cancellation, refund and dispute handling;
polling of terminal states; intake (optional hard limits, the scope check
through `apb decide`, decline texts, the one-hour rule); the per-hire cost
ceiling as run budget plus the wall-clock stop, global limits, `protection:
off`; attachments (authenticated streamed download, sha256, safe names,
worktree layout, params mapping); rework and redo as continued runs;
`nearbee hires` and `nearbee cleanup` with the deletion inventory.

Owner items: O-7 (notify channel).

Done when: native gate green; fixture tests cover success, oversized decline,
redo, restart in the middle of each write, cancellation; a killed `serve`
resumes a live draft hire without a duplicate write.

Out of scope: listings beyond the draft, tiers, evidence.

**As built (2026-11-10).** S2 task 3.4 is closed: the live fixtures are
recorded (`crates/nearbee-market/tests/fixtures/market/`, replay tests) and
the token lives in the credentials store only. The round machine of design 1
to 6 (`nearbee-core::serve::round`: reconcile, short steps, one long step,
admit), generations, lifecycle records, the one slot (D-025), abort through
`apb stop`, unresolved starts and strays; the lock file
(`nearbee-core::lock`); intake (`nearbee-core::intake`), cost and the daily
loss limit (`nearbee-core::cost`); attachments (`nearbee-core::attachments`);
rework and redo as continued runs with `feedback` and `inputs`;
`nearbee hires` and `nearbee cleanup` (`nearbee-core::retention`);
`nearbee list` without market writes (D-024). The global limit is
`limits.daily_loss_usd` only: with one active run there is no
`concurrent_runs`. Buyer messages and progress text beyond "working on it"
are not built.

Carried to the owner at the end of S3: the live checks 10.1 and 10.2 (a
killed `serve` resuming a live draft hire, a rework with feedback and a
file) and 10.3 after them; S2's x402 proof 8.2 and 8.3 (O-5, O-6) and the
billed Cloud smokes 9.1 (O-3). O-7 (notify channel) stays open: operator
notices go to serve's output.

## S4. `repo-audit-and-tiers`

Delivers: example agent A, the `repo-audit` playbook (`playbooks/repo-audit/`)
with a GitHub snapshot fetch and a findings check and render step as Nearbee
commands (`nearbee audit fetch`, `nearbee audit finish`), the findings JSON
embedded in the Markdown deliverable, and two listings materialised from the
one source with their own profile: `repo-audit-fast` (attested,
`nearbee-near`) and `repo-audit-premium` (plain, `nearbee-own-premium`); the
S2 placeholder `playbooks/first-hire` is removed. Listing files name their
tier. The `Runner` reports the execution facts of an installed playbook;
the tier check refuses a listing whose facts break its tier at publish
(before and after installing) and drifts it in `serve`, which then takes no
new work for it. The data statement is generated from the same facts,
printed for the bio, recorded with a fingerprint, snapshotted per
generation and appended to every deliverable. Attested hires collect
endpoint attestation reports before the start and after the run and
disclose them with the models apb requested per node (`crates/nearbee-cloud`).
The signed step of D-008 was specified as a contract for S5 and is built
there (`docs/privacy.md`, "Direct models and the signed call").

Owner items: O-2 (variant `repo-audit-premium`), O-3 (the fast variant's
live run on Cloud credits), O-6 (setting both listings live).

Done when: native gate green; `nearbee list` of the fast listing passes the
attested check on the live instance and records tier and fingerprint; the
premium listing is listed once its draft agent and token exist (O-2); the
premium variant's playbook ran through apb on a real public repository with
`out/findings.json`, `out/deliverable.md` and a valid manifest
(`docs/demo-log.md`); a live attestation report was fetched for both TEE
models (not billable) and recorded as a replay fixture; the fast variant's
live run and both listings going live are reported to the owner (O-3, O-6),
not done by an agent.

## S5. `doc-digest-and-paid-demo`

Delivers: example agent B, `doc-digest-private` (local conversion, Qwen3-VL
per page, `/v1/privacy/classify`, the digest through the signed step,
`digest.json`), its limits and live listing; the signed step of D-008 built
to the contract in `docs/privacy.md` ("Direct models and the signed call") as a reusable
node; the paid agent-to-agent hire of `repo-audit-fast` from the demo caller
account, with task id, settlement and the later payout in
`docs/demo-log.md`.

Owner items: O-2 (variant `doc-digest-private`), O-6 (live listing, the paid
hire's spend).

Done when: native gate green; the paid hire is accepted and recorded; the
digest's response signature verifies in the Verification Center.

Status (2026-11-11): built and tested against local `wiremock` servers and
recorded fixtures, no billed call: the Cloud client, `nearbee signed-call`,
direct and signed models through the listing, tier check, statement,
fingerprint, evidence set and run params (D-026), the four `nearbee
digest` commands, the delivery recheck with the `Response signature`
section, and `playbooks/doc-digest/` with
`listing.doc-digest-private.yaml`. Both new direct models answered a live
attestation report with one model attestation (`docs/near-ai-cloud.md`).
Open: the live answers as replay fixtures, the listing of
`doc-digest-private`, a live agent B hire and the paid A2A hire (tasks 7.1
to 7.3, owner items O-2, O-3, O-5, O-6).

## S6. `judges-path-and-submission`

Delivers: README "Try it" exact on a clean checkout; `nearbee replay` fixture
walkthrough (success, oversized decline, redo, restart, cancellation) without
credentials; fixes from the clean-checkout run; the final docs pass; the
drafted X post text; the submission payload prepared for the owner.

Owner items: O-4, O-8, O-9.

Done when: a clean clone follows the README to green; the owner has confirmed
the payload and sent the submission once (`docs/hackathon.md`).

Status (2026-11-11, implementer): built and tested, no billed call and no
market write: journal records stamped by the serve clock; apb
`1.0.0-beta.7` required (semver gate at intake and before every run start,
`external_data: true` on every market run, the `privacy_flag_missing`
incident path, the `config check` version line; compaction known off for
beta.6 and beta.7, D-023 lifted on merge); the attested tier refuses every
agent (D-027); agent A fast is `playbook.fast.yaml` with the signed
`nearbee audit review`; `nearbee replay` runs the five scenarios
(success, a decline at intake for a brief that names no repository, redo,
restart, cancellation) through the real `serve::round` and runs in CI;
README, `docs/demo-script.md`. Open: acceptance, the clean-clone run, the
quality cycle, the X post and submission drafts, relisting after the merge
(tasks 7.1 to 7.5), and the owner's live runs and submission (7.6).

## Dependencies outside this repository

1. **apb native NEAR AI Cloud support** (requested from the apb project on
   2026-11-09, `docs/apb-request-near-models.md`; specified there as the apb
   change `near-ai-cloud-models`: NEAR models through apb's built-in `dsh`
   agent with a run-scoped route, key in `NEARAI_API_KEY`, privacy class per
   model, validator warnings V220 to V222; no release date promised). Nearbee does not wait
   for it: S1 uses whatever executor path works on the day; if the apb change
   lands before S4, the profile templates and the tier validator switch to it
   in S4.

## Deferred (only if time is left after S6)

1. **`nearbee login`**: one owner sign-in over OAuth on the market's
   `/mcp/pro`, after which `nearbee list` creates the agent, sets its price
   and mints its token itself. Needs a live check first: `docs/market.md`
   says `/mcp/pro` carries agent management, the vendored worker skill lists
   no such tools (D-022).

