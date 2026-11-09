# Architecture

Nearbee is a long-running process on the owner's machine next to `apb`, plus a
CLI for publishing listings and a small demo caller. No database server: state
is files under `.nearbee/` (one durable journal per hire) and the market itself.

```
                 ┌────────────────────────────┐         ┌──────────────────────┐
  buyers ──web──▶│  NEAR AI Agent Market      │◀─x402───│ other agents (A2A)   │
  buyers ──MCP──▶│  held payment · threads    │         └──────────────────────┘
                 └─────────▲──────────────────┘
                   REST aat_ │  (polling; webhooks are v1.1 and owner-configured)
                 ┌───────────┴────────────────────────────────────────────────┐
                 │ nearbee serve                                               │
                 │  Market adapter (seam) ── Core: intake · pricing · tiers · │
                 │                           deliverable · hire journal ·     │
                 │  Runner adapter (seam) ── recovery · cleanup               │
                 └──────────────────────────────────────┬─────────────────────┘
                                                        │ REST apb_
                                          ┌─────────────▼──────────────┐
                                          │ apb dashboard (server)     │
                                          │  playbooks · profiles      │
                                          │  runs · notify · decide    │
                                          └─────────────┬──────────────┘
                                 executors: own agent CLIs │ or CLIs on NEAR AI Cloud (TEE)
```

## Modules (`crates/`)

| Module | Responsibility | Depends on |
| --- | --- | --- |
| `core/listing` | a `Listing`: materialised playbook id + version digest, tier, price, SLA, category, tags, input schema and limits, generated description, data statement | nothing external |
| `core/intake` | take or decline a hire against its listing: hard limits, scope question to a decision model, cost ceiling | `Runner.decide` |
| `core/pricing` | price and SLA from run observations; per-hire cumulative cost ceiling; the SKU expansion of one playbook into several materialised playbooks and listings | `Runner.runs`, `Runner.estimate` |
| `core/privacy` | tiers (plain, attested, blind), the profile, connector and decision-provider rules per tier, the data statement, evidence collection, deletion inventory | `Runner.profiles`, `Runner.connectors` |
| `core/deliverable` | validate the run's `out/result.json` manifest (containment, hashes, sizes; `nearbee_core::manifest` since S1), build Markdown + files, append evidence and the data statement, enforce 256 KiB in UTF-8 bytes | the local filesystem (narrow synchronous reads of the finished run's `out/`) |
| `core/hire` | the state machine of one hire with a durable journal: received → intake → start-intent → started → run-intent → running → submitted → accepted / rework / redo / cancelled / disputed → closed → deleted | `Market`, `Runner` |
| `core/recovery` | on boot and on every poll: reconcile journal vs market rows vs apb runs; resolve ambiguous starts; stop local work on cancelled or refunded hires | `Market`, `Runner` |
| `market/` | `Market` interface and its one implementation `NearAgentMarket`: REST client with retries, `clientRequestId` on sends, file upload, assignments in every status with paging | fetch |
| `runner/` | `Runner` interface and its implementation `ApbRunner` (`crates/nearbee-apb`): REST client to Nearbee's own apb instance pinned to one workspace, run start with params, worktree and budget, run lookup by the `hire_id` and `execution_id` search fields with completeness, polling; later stages add answer and review, profiles, connectors, `decide`, and `apb stats --json` over the CLI where REST lacks a route | fetch, child process |
| `cli/` | `nearbee list`, `nearbee serve`, `nearbee hires`, `nearbee cleanup`, `nearbee config check`, `nearbee replay` (fixture walkthrough) | all of the above |
| `config` | `nearbee.yaml` (committed example) + `.env` secrets | |

Interfaces are small and typed; the core never imports `market/` or `runner/`
implementations. The core does no network I/O; it may run narrow synchronous
file checks of its own (the result manifest validation reads the finished
run's `out/` with bounded reads and returns the checked bytes). Tests use in-memory fakes of both interfaces; a recorded set
of real market responses drives the `replay` walkthrough without credentials.

## Flows

### 1. Publish a listing (`nearbee list <listing.yaml>`)

1. Read the listing file: source playbook, variant (tier, profile bindings),
   `price` and `sla` (owner-set; `auto` is v1.1), category, tags, input schema,
   optional limits and scope, refund policy text.
2. **Materialise** the variant as its own apb playbook id (the REST start has
   no `version` or `overrides`): copy the source playbook into
   `playbooks/<name>/<variant>/` with fixed profile names (`nearbee-own` for
   the owner's subscriptions, `nearbee-near` for NEAR AI Cloud TEE models;
   `playbooks/README.md`), register it in the
   dedicated apb project, record its version digest in the listing. A hire runs
   that id and nothing else; `serve` checks the run's playbook id and version.
   The materialised playbook declares `supervisor.policy.trial_candidates: off`
   (publishing refuses one without it): under apb's default `next_run` policy a
   start without a version runs a supervisor's candidate version instead of
   the published one (`docs/apb.md`, "Current version and candidates").
3. Validate the variant against its tier (`docs/privacy.md`); generate the
   description, bio, data statement and refund policy text; derive the per-hire
   ceiling from the price (`docs/pricing.md`).
4. `Market.updateAgent` (name, description, bio, category, tags, sla, input and
   output schema), `Market.setPricingPlan` (per_call, price, sla). The agent
   record itself is created by the owner in the web app or over `/mcp/pro`;
   Nearbee needs its id and `aat_` token. The listing stays `draft` until the
   owner sets it live (or `nearbee list --live`).
5. Write the resolved listing to `.nearbee/listings/<handle>.json`.

### 2. Serve hires (`nearbee serve`)

1. Every 30 to 60 s, for every configured agent token: `GET
   /v1/agents/me/assignments?status=all` with paging. Classify rows by
   `deliverableUrl` and `submittedAt` (`docs/market.md`); match them to the
   journal. Rows in `cancelled`, `disputed`, `accepted` update the journal and
   stop or close local work. Acceptance is read from the market, never inferred
   from `autoAcceptAt`.
2. New first-time row not in the journal: write `received`, download
   attachments (authenticated, origin-restricted to the market host, streamed
   with size limits, safe file names, verified against `sha256`), run **intake**
   (`docs/pricing.md`: optional hard limits, then the scope check). Outcomes:
   - take: journal `start-intent`, `POST start`, journal `started`, message the
     buyer with the plan and the deadline (with `clientRequestId`);
   - decline: `POST decline {reason: rejected, detail}` within the first hour.
   Questions to the buyer before start are v1.1 (web and MCP buyers only).
3. Journal `run-intent` with the hire id and a fresh `execution_id`, then
   start the run: fresh worktree per hire, params from the brief plus
   `hire_id` and `execution_id` (both `search: true` params, added by the
   runner), `instruction`, `budget` (`docs/pricing.md`). Journal `running` with
   the run id. A start whose outcome is unknown is resolved by a search on
   `execution_id` before any retry. An empty search after an ambiguous start
   does not prove that no run exists unless the search was complete (no runs
   `not_indexed` or `withheld`, `docs/apb.md`); an incomplete empty search
   leaves the hire for recovery, never a second start.
4. Follow the run: poll `GET /api/runs/{id}`; map node stage changes to `POST
   progress` (at most one per 30 s); keep a wall-clock limit at SLA minus a
   margin; owner review gates (if the playbook has any) go to the owner through
   apb notify with a deadline, otherwise the run fails.
5. On success: validate the manifest, upload files, build the deliverable with
   the evidence block and the data statement, `POST submit` (journal
   `submit-intent` before, `submitted` after; a retried submit sends identical
   Markdown and the identical file list). On failure after start, or on a
   missing or invalid manifest: `POST decline {reason: failed, detail}` with a
   short honest reason. Never submit a placeholder, a report or a summary.
6. Rework (`deliverableUrl` set, `submittedAt` set, `in_progress`): read the
   newest buyer message, start a run with `continued_from` and the feedback as
   `instruction` in the same worktree; the per-hire cumulative cost ceiling
   applies; submit again. Dispute `redo` (`submittedAt: null`) is handled the
   same with the original brief. A `submitted` row with a fresh buyer message is
   a question about the delivered work: answered in the thread, resubmitted only
   if it reveals a fix.
7. After the market reports acceptance plus the retention window: delete the
   worktree, attachments, conversions, deliverable copies, apb run artifacts and
   CLI transcripts for that hire (the inventory in `docs/privacy.md`); journal
   `deleted`. Declined-before-start hires delete at once.

### 3. Agent-to-agent (x402)

Nothing changes on the worker side except: submit settles the payment at once,
`acceptanceWindowDays` is absent, and there is **no thread**: the A2A protocol
has no multi-turn, so listings open to callers run self-contained playbooks
only. The demo caller in `examples/` implements the x402 caller skill with
`near-api-js`: read the card, get the 402, sign the NEP-366 delegate for the
exact `accepts[]` row, resend with `returnImmediately: true`, poll `GetTask`,
print the artifact. The caller's account must not be the owner's. The owner's
payout is a later, separate step by the treasury sweep.

### 4. A playbook hiring another agent (stretch)

A `script` node in an example playbook calls the same x402 caller with a
budget param, so a Nearbee agent subcontracts a market agent and pays in USDC.
Only if time allows.

## Recovery protocol

The journal is append-only JSON lines per hire, written **before** every
external write (intent) and after it (done). On boot and on every poll cycle
`core/recovery` compares the journal with the market row and the apb run:

| Journal state | Market says | apb says | Action |
| --- | --- | --- | --- |
| `start-intent` without `started` | `startedAt` set | - | journal `started`, continue |
| `run-intent` without `running` | - | a run with the intent's `execution_id` exists | adopt it, journal `running` |
| `run-intent` without `running` | - | a complete search finds no run | start again with a new `execution_id` |
| `run-intent` without `running` | - | an incomplete search finds no run | do not start; report to the owner (absence is not proven) |
| `running` | row cancelled or refunded | run alive | pause and abort the run, journal `cancelled`, delete data |
| `submit-intent` without `submitted` | `deliverableUrl` set | - | journal `submitted` |
| `submit-intent` without `submitted` | not submitted | - | resubmit the identical deliverable |
| any | row in `disputed` | - | freeze local work, journal `disputed`; `redo` reopens |

One `serve` process owns a hire; a second instance refuses to start while a
lock file is held.

## Configuration

`nearbee.yaml` (committed, no secrets):

```yaml
state_dir: .nearbee
apb:
  url: http://127.0.0.1:7322              # Nearbee's own apb instance (scripts/apb-instance.sh)
  workspace_path: ~/.local/share/nearbee/apb-workspace   # dedicated apb project
limits:                                   # result manifest limits (S1)
  markdown_bytes: 262144
  file_bytes: 26214400
  files: 20
  total_bytes: 104857600
market:
  base_url: https://market.near.ai
agents:                                   # one entry per market agent (SKU)
  - handle: repo-audit-fast
    listing: playbooks/repo-audit/listing.fast.yaml
  - handle: repo-audit-premium
    listing: playbooks/repo-audit/listing.premium.yaml
limits:
  concurrent_runs: 2
  daily_loss_usd: 20                      # stop taking hires when failed-run spend passes it
retention:
  after_acceptance_days: 7
owner:
  review_timeout_minutes: 60
```

S1 ships the `state_dir`, `apb` and `limits` blocks
(`nearbee.example.yaml`); the other blocks arrive with their stages, and an
unknown key is refused until then. Relative paths resolve against the file's
directory; `APB_URL` in the environment overrides `apb.url`. The apb URL
must be an absolute `http` or `https` URL without userinfo
(`user:key@host`); the key goes in `APB_API_KEY`.

**As built in S2.** The credentials live in a JSON store
(`~/.config/nearbee/credentials`, mode 0600, atomic replace, wider modes
refused on read), written by `nearbee agent add` (D-022); a token-like
value in the config or a listing file is refused at load. The market client
(`nearbee-market::NearAgentMarket`) sends the token only as a bearer header
to the configured origin, retries reads with `Retry-After`, resends a write
at most once on an unknown outcome, and classes every failure as
unauthorized, refused (status plus machine code), retryable read, write
outcome unknown or protocol. The publish path
(`nearbee list`, `nearbee-apb::install`) materialises the listing's
playbook under the handle's id with `trial_candidates: off`, imports and
approves exactly that bundle, reads the installed version back, then patches
the agent and sets the pricing plan, never `listing_status`. `nearbee serve`
(`nearbee-core::serve`) polls every published listing that also has
credentials every 30 to 60 seconds (`NEARBEE_POLL_SECS`, default 45 s, clamped into the spec's bounds), takes only
`in_progress` first-time rows, and drives one hire at a time: received,
start (unless the row's `startedAt` is set), run intent with the brief as
instruction and `hire_id`/`execution_id` as params, wait with a wall clock
ending ten minutes before the row's SLA deadline, manifest validation, and
submit or decline, every external write preceded by a durable journal
record (`nearbee-core::journal::Writer`, append with fsync, consecutive
`seq`, truncated tails refuse writing). A restart resumes open intents from
the journal; a revoked token stops that one agent and keeps the others.
Worktrees live at `<apb workspace>/work/<safe hire id>/<execution>/`.

`.env`: `APB_URL` (optional), `APB_API_KEY` (if the dashboard has keys),
`NEARAI_API_KEY` (for the `nearbee-near` profile; it reaches the executors
through the environment of Nearbee's apb instance, not through Nearbee), and for the demo caller
`NEAR_CALLER_ACCOUNT_ID`, `NEAR_CALLER_PRIVATE_KEY`. Agent tokens are not in
`.env`: `nearbee agent add <handle>` stores each one in
`~/.config/nearbee/credentials` (mode 0600), keyed by handle (D-022).

## Runtime and stack

Rust, one cargo workspace: `crates/nearbee-core` (listing, intake, pricing,
privacy, deliverable, hire, recovery: no network I/O, traits `Market` and `Runner`),
`crates/nearbee-market` (the NEAR AI Agent Market client), `crates/nearbee-apb`
(the apb client), `crates/nearbee` (the CLI binary: `list`, `serve`, `hires`,
`cleanup`, `config check`, `replay`), `examples/x402-hire` (the demo caller,
`near-primitives` or `near-api` for the NEP-366 delegate; a `near-api-js`
script is the fallback if the Rust path is awkward). `reqwest`, `tokio`,
`serde`, `yaml_serde`, `dotenvy`, `clap`, `sha2`, `rust_decimal` (money is
never `f64`), `uuid` (v7 operation ids), `rustix` (descriptor-relative
`openat` for result validation without `unsafe`). Tests with `cargo test`; the core is
tested with in-memory fakes, the clients with recorded responses. The module
table above maps onto these crates. See `docs/decisions.md` D-003.

## What is deliberately not built in v1

1. No web UI. The owner watches hires in the apb dashboard (runs) and the market
   (jobs). `nearbee hires` prints the local journal.
2. No webhooks (polling covers the lifecycle; webhooks need owner-side setup
   and fast `2xx` answers, which conflict with synchronous intake).
3. No buyer questions during a run; no owner review gates in market playbooks.
4. No board bidding (v1.1: the only place with a custom price).
5. No on-chain writes by Nearbee itself.
