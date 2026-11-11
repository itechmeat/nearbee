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
| `core/tiers` (S4) | the execution facts across the seam (`Known<T>`, profile, node, route facts), `check(tier, facts, catalog)` with one English line per breach, `evidence_models` | the embedded catalog |
| `core/catalog` (S4) | the dated NEAR AI Cloud model list (`docs/reference/near-ai-cloud-models.json`) embedded with `include_str!`; which ids are owned by `nearai` | nothing external |
| `core/statement` (S4) | the generated data statement (`render`, format 1), the facts `fingerprint`, and `drift` of a listing record against the current facts | `core/tiers` |
| `core/evidence` (S4) | the `Attestor` seam, the private evidence snapshot record, the nonce, the `Endpoint attestation disclosure` renderer; serve schedules the fetches as long steps | `Attestor` |
| `core/retention` | the deletion inventory and cleanup eligibility | the local filesystem |
| `core/deliverable` | validate the run's `out/result.json` manifest (containment, hashes, sizes; `nearbee_core::manifest` since S1), build Markdown + files, append evidence and the data statement, enforce 256 KiB in UTF-8 bytes | the local filesystem (narrow synchronous reads of the finished run's `out/`) |
| `core/hire` | the state machine of one hire with a durable journal: received → intake → start-intent → started → run-intent → running → submitted → accepted / rework / redo / cancelled / disputed → closed → deleted | `Market`, `Runner` |
| `core/recovery` | on boot and on every poll: reconcile journal vs market rows vs apb runs; resolve ambiguous starts; stop local work on cancelled or refunded hires | `Market`, `Runner` |
| `market/` | `Market` interface and its one implementation `NearAgentMarket`: REST client with retries, `clientRequestId` on sends, file upload, assignments in every status with paging | fetch |
| `runner/` | `Runner` interface and its implementation `ApbRunner` (`crates/nearbee-apb`): REST client to Nearbee's own apb instance pinned to one workspace, run start with params, worktree and budget, run lookup by the `hire_id` and `execution_id` search fields with completeness, polling, `decide`; since S4 `inspect` and `inspect_source` (the execution facts, `inspect.rs`), the per-attempt models of a run (`attempt_models`), materialisation with the listing's profile; `run_params` reads a script node's run params from `APB_RUN_DIR` | fetch, child process |
| `cloud/` (S4) | `CloudAttestor` (`crates/nearbee-cloud`): the NEAR AI Cloud attestation report client behind `Attestor` (no redirects, 2 MiB cap, two attempts within 20 s, the key never in an error) | fetch |
| `cli/audit` (S4) | `nearbee audit fetch` (the GitHub snapshot of the one repository the brief names) and `nearbee audit finish` (the citation check, `findings.json`, the Markdown deliverable, the manifest), run by the `repo-audit` playbook's script nodes in the run's worktree | fetch, the local filesystem |
| `cloud/client` (S5) | `CloudClient` (`crates/nearbee-cloud/src/client.rs`): the model calls of Nearbee's own commands on `https://cloud-api.near.ai/v1` (exact-bytes completion, signature fetch, privacy classify, JSON completion); no proxy, no redirects, capped bodies | fetch |
| `cli/signed_call`, `cli/digest` (S5) | `nearbee signed-call` (one exact-bytes completion with its `provider_tee` signature, state in its out directory) and `nearbee digest pages\|extract\|classify\|compose` (agent B), run by the `doc-digest` playbook's script nodes | fetch, child process (`pdfinfo`, `pdftoppm`, `magick` under `prlimit`), the local filesystem |
| `core/signature` (S5) | the delivery recheck of a `tee_signature` manifest entry and the `Response signature` section with the Verification Center receipt link | none (pure) |
| `cli/` | `nearbee list`, `nearbee serve`, `nearbee hires`, `nearbee cleanup`, `nearbee config check`, `nearbee audit`, `nearbee replay` (fixture walkthrough) | all of the above |
| `config` | `nearbee.yaml` (committed example) + `.env` secrets | |

Interfaces are small and typed; the core never imports `market/`, `runner/`
or `cloud/` implementations: it sees only `Market`, `Runner` and `Attestor`. The core does no network I/O; it may run narrow synchronous
file checks of its own (the result manifest validation reads the finished
run's `out/` with bounded reads and returns the checked bytes). Tests use in-memory fakes of both interfaces; a recorded set
of real market responses drives the `replay` walkthrough without credentials.

## Flows

### 1. Publish a listing (`nearbee list <listing.yaml>`, as built)

1. Read the listing file: handle, name, source playbook, `price_usd`
   (decimal string), `sla_seconds`, category, tags, description, bio, and
   the optional `scope`, `protection` (default `on`) and `limits` (spec
   `listing-publish`): a protected price must leave a per-hire ceiling
   above zero (`price * 0.95 - 0.25 > 0`).
2. **Materialise** the playbook as its own apb playbook id `<handle>` with
   fixed profile names and `supervisor.policy.trial_candidates: off`, its
   version `1.0.<n>` derived from the whole bundle (the playbook text and
   every script's bytes and executable flag), approved exactly as built,
   and read back through `apb export`. A hire runs that id and that
   version and nothing else; `serve` checks the run's playbook id and
   version (`docs/apb.md`, "Current version and candidates").
3. Write the resolved listing to `.nearbee/listings/<handle>.json`
   (version, digest, price, SLA, scope, protection, limits) before the
   first market call, so a failed read never leaves the record naming an
   older version.
4. Read `GET /v1/agents/{agent_id}/pricing-plans` (the only market call)
   and print the web-app settings table: name, description, bio, category,
   tags, SLA and price from the file, each with the market value or
   "not readable with the agent token" (a draft agent answers 404), and a
   status of `ok`, `set in web app`, `mismatch` or `unverified` (a `null`
   plan SLA inherits the agent default, which the token cannot read). A
   mismatch prints and does not fail the command.
5. No market write happens: the agent token cannot change the agent
   (`403 agent_token_scope_forbidden`, measured 2026-11-10, D-024). The
   owner creates the agent and sets the texts, SLA and price in the web
   app; Nearbee needs the handle, the agent id and the `aat_` token. The
   listing stays `draft` until the owner sets it live.

### 2. Serve hires (`nearbee serve`, as built in S3)

`serve` takes the lock `<state_dir>/serve.lock` for its whole life (an
exclusive `flock`, close-on-exec so apb children never inherit it; a second
`serve` or a `cleanup` exits 4 naming the file; the kernel drops it with the
process, the file stays). Every 30 to 60 s (`NEARBEE_POLL_SECS`, measured from
the round start) it reads every served agent's assignments once (all
statuses, a 30 s read time budget) and runs one round of the core machine
(`nearbee-core::serve::round`) over all of them; each agent's writes go out
through its own market client and token. No step of a round waits for a run:

1. **Reconcile** every journaled hire against its row (design 3): a pending
   submit or decline is settled first (landed when the row shows
   `submittedAt` at or after the intent, or a new generation key; an
   identical resend only while the row still awaits it); a new row status is
   recorded once as a `lifecycle` record (`accepted` with `finalizedAt`,
   `cancelled` with its reason, `disputed` freezes the hire,
   `dispute_resolved` closes it); a closing row with an active run orders its
   abort first and the hire stays in reconciliation until the run is
   terminal. Stray runs (found by `hire_id`, not the adopted run of the
   current generation) are aborted; a stray's terminal observation records
   its cost. A rework or redo row whose generation key (the digest of
   `slaDeadlineAt`, `deliverableUrl`, `submittedAt`) is new opens the next
   generation with the feedback's digest; three polls of one rework open one
   generation.
2. **Advance** every open generation in short steps, one apb observation per
   hire (`observe_once`): a `run_observed` record only when the status or
   version changed (the terminal one carries `cost_usd`, `cost_complete`,
   `budget_usd`), the version check (a first detail without a version is
   followed for up to 60 s), a budget pause, the wall clock (the SLA deadline
   minus `min(10 min, window / 5)`), cancellation, progress (a `409 too soon`
   records a due time), then submit from the frozen
   `gen-<n>/submission.md` or decline. Every wall clock is checked here.
3. **Intake** for new first-time rows (`docs/pricing.md`): received, then
   daily loss, hard limits and cost admission, journaled one `intake` record
   per check without the brief; a decline is `rejected` with a fixed text.
   Intake runs whatever the slot, so a decline goes out within the first
   hour.
4. **At most one long step**, oldest hire first: the scope check (`apb decide
   is`, 60 s) or one attachment download (120 s, `docs/privacy.md` and spec
   `hire-attachments`), each cut off at the earliest wall clock of any hire
   with an active run (a hire whose row was not read counts by the deadline
   its run intent recorded) and retried later. The cutoff goes to the
   runner, which kills and reaps the `apb decide` child by then.
5. **Admit** at most one start when the slot is free (D-025): the market
   start of a hire whose intake is complete, then, once its files are stored,
   the run: the run body snapshot `gen-<n>/run-<e>.json` before the
   `run_start` intent, params `brief`, `inputs` (a JSON list of the stored
   files) and for a rework or redo `feedback` (the full latest buyer message,
   empty for a redo) with `continued_from` the hire's latest run; `budget.usd
   = ceiling - spent` (none with `protection: off`). Before a continued run
   the previous `out/` moves into `gen-<n-1>/out/` and a fresh one is made.

One worktree per hire, `<apb workspace>/work/<safe hire id>/`, shared by its
generations (`inputs/` and the previous `out/`); an S2 hire keeps the
per-execution path its run intent journaled. A script node that keeps
state of its own keeps it per execution inside the worktree: agent B's
commands use `digest/<execution_id>/` (`src/`, `pages/`, `pages.json`,
`pii.json`, `signed/`, `usage.json`), so a retried node of the same
execution resumes and a rework or redo (a new execution id) starts fresh
and never reads an earlier execution's directory; `nearbee cleanup`
deletes it with the worktree. The generation's frozen direct models reach
the run as the `direct_models` param (reserved like `hire_id` and
`execution_id`).

**Generations.** Every journal record carries `generation` (absent reads as
1, so S2 journals stay valid). A generation's state is derived from its own
records only; a `decision` closes only its generation, the hire is closed by
a `lifecycle` record and frozen by `disputed`. Intents: `market_start`,
`run_start`, `market_submit`, `market_decline`, `run_abort`,
`market_progress`, `scope_check`.

**The one slot (D-025).** Taken by an open run start, a run observed
non-terminal, an open abort (closed hires included) and by any hire whose
journal cannot be read. The slot is derived from every journal on disk
right before admission, and waiting hires are admitted oldest first. A
listing of the hires directory that fails holds the slot (and leaves the
daily loss unknown, so that intake check waits); the operator is told
every round. The same predicate keeps a closed hire out of `nearbee
cleanup` while its local work (an adopted run still running, an open abort,
an unresolved start) is not settled. Every
apb start waits for it; a waiting first-time hire past its deadline is
declined `rejected` when it was never started on the market, `failed` when
the owner or its own market start already started it (work counts as
started), and a waiting rework or redo `failed`. A definitely refused start closes its run intent and frees the
slot (an S2 journal's refused start, recognised by its `run start refused`
decline, counts as closed too).

**Stop.** `Runner::abort` runs `apb stop <run>` with Nearbee's
`APB_CONFIG_DIR` from the workspace (30 s); the `run_abort` intent closes
when an observation reads terminal, and the abort repeats every round until
then. An abort that stays unconfirmed never holds back the hire's other
local work (its start lookup, its hire lookup, its adopted run's wall
clock), and a decline already queued never prevents the abort of its
generation's run.

**Recovery without a listing.** When no listing with credentials loads,
`nearbee serve` still runs its rounds while any journal on disk holds local
work (the slot predicate) and exits with "nothing to serve" only once that
work is settled: stopping a run needs neither a listing nor a token.

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

## Recovery protocol (as built in S3)

The journal is append-only JSON lines per hire, written **before** every
external write (intent) and after it (result); frozen bytes (a submission, a
run body) are written to `gen-<n>/` (0700 and 0600, temp, fsync, rename)
before their intent. Every round re-derives each hire's state from its
journal, so a restart and a first serve run the same code.

| Journal state | Market says | apb says | Action |
| --- | --- | --- | --- |
| `market_start` open | `startedAt` set | - | close it (`already started`), go on |
| `market_start` open | not started | - | resend the idempotent start |
| `run_start` open, no run | - | one run with the `execution_id` | adopt it |
| `run_start` open, no run | - | several runs | adopt none, `attention start_ambiguous` |
| `run_start` open, no run | - | none (complete or not) | start nothing; `attention start_unresolved` after 120 s; decline `failed` at the deadline; the slot stays taken and the lookup keeps running |
| run observed, not terminal | row closed (cancelled, disputed, resolved, accepted) | run alive | `run_abort`, abort every round until terminal, then the lifecycle closes |
| run terminal, no submit | in progress | - | validate `out/`, snapshot, `market_submit` intent, submit |
| `market_submit` open | `submittedAt` at or after the intent, or a new generation key | - | close it as landed, no resend |
| `market_submit` open | in progress, same key, not submitted | - | resend the snapshot byte for byte; a missing or mismatching snapshot sends nothing (`attention snapshot_mismatch`) |
| `market_decline` open | cancelled | - | close it |
| `scope_check` open | - | - | close it as skipped (`interrupted`), never asked again |
| a later run found by `hire_id` | - | not the adopted one | abort it, record its cost, never submit it |
| `run_start` open, execution lookup failing | - | runs found by `hire_id` | a run whose params name the unresolved execution (or whose detail cannot be read) is left to that lookup; every other run is aborted or recorded |
| journal unreadable | - | - | nothing for that hire, the slot is held, the operator is told |
| S2 journal, `run_start` or `market_submit` open | - | - | `attention s2_pending`, nothing sent (a start holds the slot) |

**Operator procedure: an unresolved start** (`attention start_unresolved`).
Serve never starts a second run for a generation whose start is unresolved.
Check apb for a run of the hire (`nearbee runs find --hire-id <id>` and
`--execution-id <id>`). If a run exists, the next round adopts or aborts it
on its own. If none exists and the operator is sure none will land (apb was
restarted since, or its run directory shows no run), stop `serve` and append
a `result` record for the open `run_start` intent with `outcome: "operator"`
and the next `seq`, keeping the record's `generation`; then start `serve`.

**Operator procedure: `s2_pending`.** An S2 journal's open intent has no
snapshot to resend from. Check the run in apb (`nearbee runs find`) and the
row on the market (the web app or `GET /v1/agents/me/assignments`). For an
open `run_start`: adopt by appending a `run_observed` record with the run id,
or close it with a `result` as above when no run exists. For an open
`market_submit`: when the row shows the submission, append a `result` with
`outcome: "submitted"`; otherwise append a `result` with `outcome:
"operator"` and let serve decline or redo the work. Stop `serve` before
editing a journal; every appended line keeps `schema_version`, the next
`seq`, the `hire_id` and the generation.

**Known limitations (S3 review, accepted).** Recovery is proven for a
process crash or restart at every journal write boundary. Two gaps remain
that need a power loss together with a failed directory sync (a filesystem
fault or unusual permissions on the state directory):

1. The directory-creation barrier caches a directory as durable once its
   own parent is synced. If a sync higher up the path fails, a later call
   does not retry it. After a power loss, a hire directory and its journal
   could then be lost, and the hire would be seen as new.
2. When cleanup removes a `gen-<n>/` snapshot but cannot sync the hire
   directory, the retry plans only the snapshots still present. It can
   record the deletion as done without that sync. After a power loss, the
   snapshot could come back while `nearbee hires` reports the hire deleted.
   Check with `ls <state_dir>/hires/<hire>/` after a cleanup that reported
   a sync failure.

## Configuration

`nearbee.yaml` (committed, no secrets):

```yaml
state_dir: .nearbee
apb:
  url: http://127.0.0.1:7322              # Nearbee's own apb instance (scripts/apb-instance.sh)
  workspace_path: ~/.local/share/nearbee/apb-workspace   # dedicated apb project
  config_dir: ~/.local/share/nearbee/apb-config          # its APB_CONFIG_DIR, for apb stop, apb decide and nearbee list
  xdg_config_home: ~/.local/share/nearbee/xdg-config     # its unit's XDG_CONFIG_HOME, holding no OpenCode config (S4)
limits:                                   # result manifest limits (S1) and the daily loss limit (S3)
  markdown_bytes: 262144
  file_bytes: 26214400
  files: 20
  total_bytes: 104857600
  daily_loss_usd: "20"                    # stop taking hires for the UTC day once failed runs cost this
retention:
  after_acceptance_days: 7
```

These are the keys as built in S3 (`nearbee.example.yaml`); an unknown key
is refused. Listings are not named in the config: `serve` serves every
resolved listing record under `<state_dir>/listings/` that also has
credentials. Relative paths resolve against the file's
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
Worktrees lived at `<apb workspace>/work/<safe hire id>/<execution>/` in
S2; S3 moved them to one directory per hire (flow 2).

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
