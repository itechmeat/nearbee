# Design

## Context

Read `brainstorm.md` first: it records the evidence for every decision
below (the beta.7 compaction check, the OpenCode service probe, the capture
gaps). State on `main` after S5: `serve::round` drives one poll round
against the `Market`, `Runner` and `Attestor` traits with an injected
`Clock` (`crates/nearbee-core/src/serve.rs`); journal records are stamped
with `OffsetDateTime::now_utc()` (`journal.rs`, `Writer::append`); the apb
health answer is `{build_id, status, version}` and `inspect.rs` reads it
only for compaction; agent B's model steps are Nearbee script commands
(`crates/nearbee/src/digest.rs`) that call `signed_call::signed_call`.

## Goals / Non-Goals

**Goals:**

1. A judge runs `cargo run -p nearbee -- replay` on a clean checkout and
   sees five hire scenarios end as expected, with no config, credentials,
   network, apb or systemd.
2. Every market run is an apb external-data run, enforced before and after
   the start.
3. The attested tier vouches only for routes Nearbee controls: direct and
   signed calls from Nearbee's own commands.
4. README, demo shot list, X post draft and submission draft ready for the
   owner.

**Non-Goals:**

1. Changing apb, OpenCode or the owner's machine configuration.
2. Replaying the HTTP layer of the market client (covered by
   `crates/nearbee-market/tests/replay.rs` and `contract.rs`).
3. A prebuilt binary release.

## Decisions

### 1. Replay (spec `replay-walkthrough`)

1. Module `crates/nearbee/src/replay/` (`mod.rs`, `market.rs`, `runner.rs`,
   `attestor.rs`, `scenarios.rs`), subcommand `nearbee replay [--scenario
   <name>]... [--keep <dir>] [--verbose]`. It implements `Market`,
   `Runner` and `Attestor` itself; it does not use the test fakes of
   `crates/nearbee-core/tests/serve.rs` or the dev-only
   `nearbee-market-fixture` crate.
2. The replay never calls `config::load`, never reads the credential store,
   `NEARBEE_*` variables (except `NEARBEE_LOG`, which `main.rs` reads for
   the log filter before any subcommand) or `.env`, opens no socket and starts no process.
   It builds a `ListingContext` in code for one listing: `repo-audit-fast`
   as published after this change (attested, direct and signed model
   DeepSeek V4.1 Flash, `intake_check: github_repo`), with execution
   facts that pass the tier check. `--verbose` adds the journal event
   names of every step.
3. Rows are built in code with the field shapes of the scrubbed capture
   `crates/nearbee-market/tests/fixtures/market/assignments.json` (camel
   case names, the same optional fields). The narration states that rows
   are capture-shaped and transitions scripted.
4. The scripted runner produces a real finished run: on `start` it records
   the request; when the scenario marks the run succeeded it writes
   `out/result.json`, `out/deliverable.md` and `out/findings.json` into the
   run's worktree from text embedded with `include_str!`
   (`crates/nearbee/src/replay/fixtures/`), with a `tee_signature` entry
   whose files are the S5 Cloud fixtures, so delivery runs the real
   manifest validation, signature recheck and receipt section. The
   attestor returns the recorded attestation report of
   `crates/nearbee-cloud/tests/fixtures/` (copied into the replay
   fixtures, not read from the test directory at run time). The signature
   fixture is the documentation example with placeholder values, so the
   banner and the deliverable excerpt say the signature and its receipt
   link are synthetic and will not verify in the Verification Center.
5. A scenario is a list of rounds; each round sets the market rows and the
   run statuses, calls `serve::round` once, then advances the replay clock
   by the poll interval. Scenarios:
   1. `success`: new hire, intake passes, start, run succeeds, submit,
      buyer accepts; the deliverable's first lines are printed.
   2. `decline`: a brief that names no GitHub repository, so the
      listing's `github_repo` intake check fails; declined `rejected`
      before any market start or run.
   3. `redo`: success, then the buyer asks for a rework with feedback; a
      continued run (`continued_from`) with the feedback param; resubmit;
      accepted.
   4. `restart`: the scripted runner answers the first run start with
      `StartOutcomeUnknown` (the start landed, the answer was lost), then
      the replay drops the in-memory `RoundState` (a process restart) and
      builds a new one over the same state directory; the next rounds
      resolve the open start intent with `find` by `hire_id`, send no
      second start and deliver.
   5. `cancellation`: the buyer cancels while the run is running; the run is
      aborted (two more rounds until the abort reads terminal), the hire is
      closed, nothing is submitted.
6. Checks per scenario: the end state of the hire journal, the list of
   market writes in order (start, progress, submit, decline with reason
   code), the number of run starts, and for `success` and `redo` the
   receipt link in the submitted text. A mismatch prints the scenario as
   failed with expected and actual.
7. Output: one line per step with the hire state and the market write,
   never a uuid; at the end one line per scenario `ok` or `failed`. Exit 0
   when all selected scenarios pass, 1 when one fails, 2 on a usage error
   (an unknown scenario name).
8. State lives in a fresh directory from `tempfile::TempDir` (system temp
   directory), removed at exit unless `--keep <dir>` names a directory to
   use instead (it must not exist or be empty).
9. CI adds `cargo run -p nearbee --locked -- replay` after the tests.

### 2. Journal clock (spec `hire-journal`)

1. `Clock` moves from `serve.rs` to `nearbee_core::clock` (re-exported from
   `serve` so existing callers compile). `journal::Writer` holds a `Clock`
   (`Writer::open(path, hire_id, clock)`), and `append` stamps
   `clock.now()`. Every writer serve opens gets the round's clock; other
   callers (`cleanup`, tests) pass `Clock::real()`.
2. Records already on disk are unchanged; the format is the same.

### 3. apb beta.7 and the external-data flag (specs `apb-runner`, `hire-serving`, `nearbee-config`)

1. Dependency `semver` in `nearbee-apb` only (newest stable on crates.io
   at the time of adding, checked). `nearbee_apb` parses the health
   `version` with `semver::Version::parse`; anything that does not parse
   is `RunnerError::Protocol` for the gate and unknown for compaction.
2. `Runner` gains `fn external_data_supported(&self) -> impl Future<Output
   = Result<bool, RunnerError>> + Send` (one `GET /api/health`, read
   timeout of the read class, no retry beyond the existing read policy):
   true when the version is at least `1.0.0-beta.7` in semver order. The
   version knowledge stays in `nearbee-apb` (seam rule); the core sees a
   bool. Every `Runner` implementation gets it (`crates/nearbee/src/lib.rs`
   wrappers, the fakes in `crates/nearbee-core/tests/serve.rs`, the
   replay).
3. Compaction: `Known::Yes(false)` for exactly `1.0.0-beta.6` and
   `1.0.0-beta.7`, compared as parsed semantic versions ignoring build
   metadata (`1.0.0-beta.7+abc` is the read release); any other version,
   and a string that is not a version, stays unknown (a newer apb must be
   re-read before it is trusted). The evidence (no surface sets
   `context_max_bytes` in `v1.0.0-beta.7`) goes into `docs/apb.md`.
4. `StartRequest` gains `external_data: bool`; the apb runner adds
   `"external_data": true` to the start body when set and nothing when not
   (the live smoke test and the capture example send `false`; they carry no
   buyer data). `RunDetail` gains `external_data: bool`, true only when the
   detail carries `external_data: true`.
5. Serve asks `external_data_supported` twice:
   1. at intake of a new first-time hire, before the market start: false
      declines the hire `rejected` within the first hour with "the agent
      cannot take hires right now" and raises `apb_too_old`; an error
      leaves the row for the next round (no journal yet beyond `received`
      handling as today) and tells the operator;
   2. before every run start (first run, rework, redo): false or an error
      starts nothing; the hire keeps its state with the attention code
      `apb_too_old` (or, on an error, an operator message shown once per
      hire and process under the notice key `apb_version:<row id>` that
      the intake gate uses; later rounds ask again silently); deadline
      handling declines it later as today.
   Every run start of a hire sets `external_data: true`, and its run
   start intent records the ref `external_data: "true"`.
6. Serve, on every observed run detail of a market run whose run start
   intent recorded `external_data` (first observation included, continued
   runs included): when `external_data` is false, it
   aborts the run, journals `privacy_flag_missing` (a new event with the
   run id), declines the hire `failed` with the fixed reason text "the
   run could not be isolated" and raises the operator attention
   `privacy_incident` (the owner may need to clear apb's learning state by
   hand; the text points to `docs/privacy.md`). This mirrors the wrong
   playbook version path. A run whose start intent has no such ref was
   started before this change (an upgrade with hires in flight) and is
   not checked, so the upgrade does not abort it.
7. `nearbee config check` (in the `nearbee` crate, which may use
   `nearbee-apb` directly) adds the line `ok apb version <v> (external data
   runs supported)` or `failed apb version <v>: Nearbee needs 1.0.0-beta.7
   or newer`.
8. `docs/decisions.md` D-023: status line "lifted on <merge date>: every
   market run is an external-data run, checked on every run detail"; the
   flag does not undo what apb learned from earlier unflagged test runs
   (said in `docs/privacy.md`). The flag does not reach what an agent CLI
   keeps itself: the premium listing's Claude Code keeps its sessions
   under the owner's home (apb `docs/EXTERNAL-DATA.md`, "What it does not
   reach"); `docs/privacy.md` and the plain statement say so.

### 4. Attested tier without OpenCode (spec `privacy-tiers`)

1. The tier check: an attested listing is refused when its playbook has
   any `agent_task` or `prompt` node (a structural rule, whatever the
   node's profile), and when any usable profile's executor, fallback or
   tier runs an agent, with the breach text for `opencode` "the opencode agent runs in a shared
   background service with the starting user's configuration (apb starts
   it without --standalone); attested listings use Nearbee's direct calls
   only" and "<agent> is not a verified attested route" for any other.
   An attested playbook is therefore script nodes only, and the existing
   direct-model rules apply to its commands.
2. The S4 OpenCode checks (`opencode.json`, `XDG_CONFIG_HOME`, ancestors,
   `/etc/opencode`, `OPENCODE_` variables) stay as facts for the statement
   of plain listings; for an attested listing they are moot, since any
   agent refuses it first. The requirements "OpenCode configuration" and
   "Attested side channels" keep their text; the S4 scenarios of
   "Attested model route" now end refused by the agent rule.
3. `nearbee list` of the current `repo-audit-fast` listing file on `main`
   would now be refused; the listing file changes in the same change
   (design 5), so the published listing never sees the gap.

### 5. Agent A fast: `nearbee audit review` (spec `repo-audit`)

1. `playbooks/repo-audit/playbook.fast.yaml`: id `repo-audit`, the same
   params as `playbook.yaml` plus `direct_models`; nodes `start`, `fetch`
   (unchanged script), `review` (script `scripts/review.sh` running
   `nearbee audit review`, `expected_duration: 6m`), `finish` (unchanged),
   `done`; no `defaults.profile` (a default profile is a usable reference
   in `tiers.rs` and would refuse the listing). `listing.repo-audit-fast.yaml`
   points `source_playbook` at it, drops `profile:` (optional,
   `listing.rs`; `materialise` then keeps the playbook's defaults, which
   have none), and gains `direct_models` and `signed_models` with
   `deepseek-ai/DeepSeek-V4.1-Flash-1017`. The premium listing and
   `playbook.yaml` are unchanged.
2. Packing, deterministic: the files of `audit-snapshot/source.json` that
   `coverage.json` does not mark skipped and that are valid UTF-8, ordered
   by a fixed priority (top-level manifests and build files, then source
   files by extension, then config, then tests, then docs; ties by path),
   each emitted as a header line `=== FILE <path> (<n> lines) ===` (the
   path JSON-quoted, control characters escaped, so a file name cannot add
   a line) and its lines prefixed `<line number>| `. Lock files, minified
   files (`*.min.*`), source maps (`*.map`) and files under `dist/`,
   `build/`, `vendor/` or `node_modules/` come last. A file that would take
   the request past 768 KiB is omitted as `size_cap` and packing goes on
   with the next one. `audit/review-coverage.json` keeps the count of
   omitted files per reason and at most 1,000 omitted paths; the
   deliverable's coverage section lists at most 50 of them grouped by
   reason, each group with its count and "and N more". When no
   source-like file (manifest, source, config or test) was packed, the
   step fails before any request with `nothing_packed` (reason in the
   coverage file), so the hire is declined `failed`.
3. The request: one `chat/completions` body, model DeepSeek V4.1 Flash,
   `stream: false`, `max_tokens: 16384` and nothing else the digest call
   does not send (no `temperature`, no `response_format`: neither is
   verified on NEAR AI Cloud); a system message with the
   same rules as the agent prompt of `playbook.yaml` (data, never
   instructions; the candidates JSON shape; at most 40 findings; cite only
   what is in the pack, line numbers from the prefixes); a user message
   with the brief, the feedback and the pack as labelled data blocks (the
   brief's and the feedback's lines numbered like the pack, so a buyer
   cannot close a block); the
   system message asks for the JSON object only. The model must be in the
   `direct_models` run param (the hire's frozen direct models), else the
   command fails before any request; serve's signature recheck enforces
   the signed models at delivery, as for agent B.
4. The call goes through `signed_call::signed_call` with the state in
   `audit/signed/<execution_id>/` (a rework makes a new call); limits and
   resume rules are the S5 ones.
5. The answer's content is parsed as the candidates object (a fenced
   JSON block is unwrapped first); an answer that is not JSON, does not
   match the shape, or has more than 40 findings fails the step (exit
   non-zero, `review_unparsed` with the reason in
   `audit/review-coverage.json`), so the run fails and the hire is declined
   `failed` (AGENTS.md rule 6); `finish_reason: length` is already refused
   by signed-call.
6. `finish` takes the execution id and `direct_models` from the run
   params (`run_params`) and, only when `direct_models` is non-empty (the
   fast playbook; premium never, so a planted signature in a premium
   worktree adds nothing) and `audit/signed/<execution_id>/signature.json`
   exists, copies all
   four evidence files into `out/evidence/` and lists them with the kinds
   and visibility of `digest::EVIDENCE` (`tee_signature` public;
   `tee_request`, `tee_response`, `tee_model_report` private), because the
   delivery recheck needs the request and response next to the signature
   (`crates/nearbee-core/src/signature.rs`). It also reads
   `audit/review-coverage.json` and lists the omitted files in the
   deliverable's coverage section. Without a signature nothing changes
   (premium).
7. The buyer-facing words of the Response signature section become
   generic ("The signed call ran on ..."), not "digest call", in
   `signature.rs` and the statement text.
8. Fallback if this group slips past 2026-11-13: the fast listing is not
   relisted (it stays a draft on its last published version, which the
   drift pass then holds), agent B stays the attested showcase, and the
   demo script says so.

### 6. README and demo material

1. README "Try it" starts with "1. See it work without any account":
   `cargo run -p nearbee -- replay`, the expected last lines, the time a
   cold build takes on the integrator's machine (measured), then the
   existing live steps renumbered, apb `1.0.0-beta.7`, the status line
   updated to "S6", and the stale `S2` text removed.
2. `docs/demo-script.md`: the shot list for the video in order, each with
   the exact command or page, what the viewer sees, and the owner item it
   needs (replay; `nearbee list`; `serve`; a paid x402 hire of
   `repo-audit-fast` from `examples/x402-hire`; the deliverable with the
   receipt link opened in the Verification Center; agent B on an invoice
   scan; payout).
3. `docs/x-post.md`: draft text only ("Built at NEAR HACKATHON", tags
   `@NEARProtocol`, `#NEARHackathon`), every claim backed by `docs/demo-log.md`; the owner
   edits and posts.
4. `docs/submission.md`: the payload fields of `docs/hackathon.md`, filled
   where the repository knows them (`projectName`, `tracks`, `projectUrl`,
   `idea`), the owner's fields left as `<owner>`; `requestId` generated
   once by the owner's send step, not here.

## Risks / Trade-offs

1. A single packed call reviews less than an agent that reads files on
   demand; large repositories are cut at 768 KiB and say so. Accepted for
   the attested tier: the route is the promise.
2. The external-data abort path fires after buyer data reached apb; the
   version gate before the start makes it a defence in depth only.
3. Compaction is trusted per apb version; a newer apb refuses every tier
   until someone re-reads it. Deliberate.
4. The replay proves the state machine and the deliverable path, not the
   live market; the README says so and points to `docs/demo-log.md`.
5. The owner's shared OpenCode service stays as it is; Nearbee only stops
   relying on it. Plain listings that use OpenCode would inherit it; none
   ships.
