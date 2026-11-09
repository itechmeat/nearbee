# Design: market and first hire

## Context

See `proposal.md` and `brainstorm.md`. Market facts: `docs/market.md`, the
vendored worker skill (`reference/agent-api.md`, `flows/worker-lifecycle.md`,
`flows/messaging.md`), the x402 caller skill, `docs/reference/market-openapi.json`.
Where the live market differs from these, the live market wins and
`docs/market.md` records it.

## Goals / Non-Goals

Goals: one real hire end to end; no duplicate apb run or submit across a
restart; tokens never in the repository, config or logs; an x402 payment
signed and proven without spend.

Non-goals: rework, redo, questions, intake, attachments, cancellation and
refund handling, settled payments, more than one listing.

## Decisions

### 1. Where the code lives

| Module | Contents |
| --- | --- |
| `nearbee-core::market` | `Market` trait, row and request types, classification (pure) |
| `nearbee-core::credentials` | credentials file read and write (std fs, 0600 check) |
| `nearbee-core::listing` | listing file types and validation |
| `nearbee-core::journal` | S1 types plus the writer (append, fsync, reopen) |
| `nearbee-core::serve` | the serve state machine over `Market` and `Runner`, one hire step at a time, testable with in-memory fakes |
| `nearbee-market` | `NearAgentMarket` (reqwest), capture example, fixtures |
| `nearbee-apb` | `install_playbook` (bundle import and approval through the `apb` CLI, the logic of `scripts/apb-instance.sh install` moved into Rust; D-004 allows the CLI where REST has no route) |
| `nearbee` | commands `agent add`, `list`, `serve` |
| `examples/x402-hire` | Node script, own `package.json`, `near-api-js` v7, outside cargo |

### 2. Market client

Base URL `MARKET_URL` (default `https://market.near.ai`). Same transport as
`ApbClient`: rustls, no redirects, no proxies, `retry(never)`, connect
timeout 5 s; per call 15 s for reads, 30 s for writes. Retries as in the
spec. `Market` methods: `assignments(agent) -> Vec<Row>`, `start`,
`message(clientRequestId)`, `progress`, `submit(markdown, files)`,
`decline(reason, detail)`, `agent(handle)` (public), `patch_agent`,
`pricing_plans`, `create_pricing_plan`, `make_default`, `upload_file`
(reserve, PUT the exact bytes to the presigned URL with no extra headers,
complete). Redaction removes `aat_` tokens, the configured tokens and the
token segment of `/files/<token>/` links, and the query of presigned URLs.

### 3. Fixtures

`crates/nearbee-market/tests/fixtures/market/*.json` in the S1 fixture
format, captured by `cargo run -p nearbee-market --example capture -- <name>`
with the token from the credentials store. Captures needing no hire come
first; the ones needing a hire are taken during task 7 (the owner's web
hire) and a second owner hire for `decline rejected`. Scrubbed: tokens,
capability links, presigned URL queries, emails, the brief text (replaced
with a synthetic one of the same shape).

### 4. Credentials

JSON `{ "<handle>": { "agent_id": "...", "token": "aat_..." } }`; written to
a temp file in the same directory with mode 0600, fsync, rename. Read
refuses mode bits `0o077`. The token is read from stdin to EOF, trimmed,
must match `^aat_[0-9a-f]{32}$`. The token held in memory is a `Secret`.

### 5. Listing and materialisation

The source playbook for S2 is `playbooks/first-hire/` (a trivial
`agent_task` on `nearbee-own` that writes a short Markdown reply to the
brief, plus the S1 manifest script). Materialisation writes a copy with
`id: <handle>`, profile names unchanged, `supervisor.policy.trial_candidates:
off` set, and a version derived from the source digest, imports it as a
bundle into Nearbee's apb instance and approves that exact bundle (preview
must say unchanged before approval, as in S1). The resolved listing
(`handle`, `agent_id`, playbook id, version, digest, price, SLA, published
time) goes to `.nearbee/listings/<handle>.json`.

### 6. Serve

One loop, poll interval 45 s (config, validated into the spec's 30 to 60 s),
agents from the published listings that also have credentials. Per hire,
journal records in order: `Received{row digest, brief sha256 and length}`,
`Intent{start}` / `Result{startedAt}` (skipped when `startedAt` already set),
`Intent{run, execution_id, worktree}` / `RunObserved{run_id}` / `Result`,
then `Intent{submit, request digest, worktree}` / `Result` or
`Intent{decline, reason kind, plain-text detail, buyer reason}` / `Result` and
only then the `Decision`. Round 3: the replay is one explicit state machine.
Every round derives the hire's state from its journal alone (`New`,
`StartIntentOpen`, `Started`, `RunIntentOpen`, `Running(run_id)`,
`RunFinished(status)`, `SubmitIntentOpen`, `DeclineIntentOpen(reason)`,
decided states) and applies that state's single transition, then derives
again; a restart and a first serve run the same code. Priority in the
derivation: a decision first; then open submit and decline intents, which
win over a run intent (no second deliverable, no second decline); then the
run; then the start. Every terminal outcome writes a decision exactly once;
a definitely refused decline writes `left` once and is never resent. A
decision or a closed intent never hides open work: after a restart a run
with an observed run id is followed even when its run intent is closed, and
an unknown write outcome leaves its intent open for the next round. Worktree:
`<apb workspace>/work/<safe_id>/<execution>/` (apb requires it inside the
project); a journaled worktree that differs from the computed one is never
trusted. Wall clock: the row's `slaDeadlineAt` minus 10 minutes (the SLA
clock starts at award, not at intake), or the listing's `sla_seconds` from
the row's `startedAt`, else the locally recorded start time; a missing or
zero SLA is no deadline; on expiry the run is paused
(apb's predictive stop) and the hire declined `failed`. Progress: one
message at run start ("working on it"), at most one per 30 s. On start,
serve replays every journal: open intents are resumed per the spec, a
complete-and-empty run lookup proves the start never landed and starts the
run with the same execution id. One broken hire (unreadable journal,
refused write) is reported for the operator and never blocks the other
hires. The brief reaches the playbook only as the untrusted `brief` param
(rule 3); the instruction is fixed text. Before any submit the deliverable
is scanned: an `aat_` token, a configured secret or a stored agent token
refuses it (decline `failed`, generic buyer reason, detail in the journal).
Decline reasons sent to the buyer are short and generic; paths and apb
internals stay in the journal. Hires are processed one at a time (S1 runs
serially).

`serve --reject <handle>`: every new first-time row of that agent is
declined `rejected` with the detail "test decline, nothing is charged", after
journaling it.

### 7. x402 example

`examples/x402-hire/index.mjs` per the caller skill's snippet: agent card,
unpaid call, 402 row, signing via `near-api-js` v7's own NEP-366 delegate
helpers (`KeyPairSigner.signDelegateAction`), local verification: the
derived public key is queried with `view_access_key` (the answer has no
`public_key` field, so the key is proven by the query itself) and must be
full access, the nonce is the access key nonce plus one, and the signature
is verified independently over the recomputed NEP-366 hash. Guard rails
before anything is signed: a price ceiling (`--max-usd`), a USDC contract
allowlist per network, a handle shape check, https-only A2A and RPC URLs,
and a cap on `maxTimeoutSeconds`. Send with `returnImmediately: true`,
print task id; then `GetTask` polling until a terminal state. Env:
`NEAR_CALLER_ACCOUNT_ID`, `NEAR_CALLER_PRIVATE_KEY`, `NEAR_RPC_URL`
(default mainnet), read from the process or `examples/x402-hire/.env`
(ignored). `--dry-run` stops after local verification and sends nothing.

No-spend proof (owner-gated): the owner sets `repo-audit-fast` live at the
lowest price; `serve --reject repo-audit-fast` runs; the example pays; the
hire arrives, is declined `rejected`; `GetTask` shows the refund; the owner
may set the agent back to draft. Recorded in `docs/demo-log.md` and
`docs/market.md`.

### 8. cargo deny

`deny.toml` with advisories (deny), licenses (allow the set actually in the
tree, at least MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib), bans and
sources defaults; CI installs `cargo-deny` with a pinned version.

## Risks / Trade-offs

1. [The live market differs from the vendored docs] -> capture first, code
   against fixtures, record differences.
2. [The owner's hire or the live toggle is not available when reached] ->
   the implementer stops at that task and reports; everything else is
   offline-tested.
3. [Moving the journal writer forward from S3] -> S3 builds the recovery
   table and lock file on it; the format is S1's.
4. [A rejected x402 hire might still hold funds] -> the proof is read from
   `GetTask` and the caller's USDC balance before and after; if anything is
   held, the owner is told at once.

## Migration Plan

`NEARBEE_TOKEN_REPO_AUDIT_FAST` moves from `.env` into the credentials store
with `nearbee agent add` (task 3), then the line is removed from `.env`.

## Open Questions

None that change specs or tasks.
