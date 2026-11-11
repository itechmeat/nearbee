# Design

## Context

See `proposal.md` for the why and `brainstorm.md` for the chosen approach.
The serve machine of S3 (`crates/nearbee-core/src/serve.rs`) is where every
new step plugs in; its recovery rules are strict, so this design adds steps
at fixed points only: a drift pass before intake (pass 2b), a start
preflight before the market start and the apb start (statement snapshot,
start evidence), a long step for evidence fetches (pass 3, beside scope
checks and downloads), and the assembly in delivery. apb is
`v1.0.0-beta.6`; facts about it below were read from its source at that
tag.

## Goals / Non-Goals

**Goals:** the decisions below, each small enough to implement and test
without inventing protocol.

**Non-Goals:** verifying attestation reports cryptographically (collected
and disclosed, not verified); the signed step's code (S5); anything the
proposal lists out of scope.

## Decisions

### 1. Listing files and materialisation

1. `Listing` (`crates/nearbee-core/src/listing.rs`) gains
   `tier: Tier` (`plain` | `attested`, required, serde lowercase),
   `profile: Option<String>` (1 to 64 of `[a-z0-9-]`) and
   `intake_check: Option<IntakeCheck>` (only `github_repo`).
   Every checked-in listing file and every Rust initializer of `Listing` is
   updated in the same group (the S2 placeholder gets `tier: plain`).
2. `materialise` (`crates/nearbee-apb/src/install.rs`) gains
   `profile: Option<&str>`. When set, it parses the source with
   `yaml_serde` into a `yaml_serde::Value`, refuses any node mapping that
   has a `profile` key (error names the node id), sets `defaults.profile`,
   emits the YAML, then runs the existing line-based steps (id rewrite,
   trial setting). The version is derived after this rewrite. No other text
   heuristic is added. Without a profile the output is unchanged.
3. `ResolvedListing` gains, all `#[serde(default)]`: `tier`,
   `intake_check`, `statement`, `statement_sha256`, `statement_format`,
   `fingerprint`, `evidence_models: Vec<String>`. A record is serveable
   only with tier, fingerprint, statement, a matching statement hash and
   `statement_format == STATEMENT_FORMAT`.

### 2. Execution facts across the seam

New module `crates/nearbee-core/src/tiers.rs`, plain data deriving
`Debug, Clone, PartialEq, Eq, Serialize`:

```rust
pub enum Known<T> { Yes(T), Unknown(String) }       // reason when unknown
pub struct ProfileRef { pub name: String, pub scope: String }  // "project" | "global" | "auto"
pub struct ExecutorFact { pub agent: String, pub model: String }
pub struct ProfileFact {
    pub reference: ProfileRef,
    pub executor: ExecutorFact,
    pub fallbacks: Vec<ExecutorFact>,      // declaration order kept
    pub tiers: Vec<(String, ExecutorFact)>,// declaration order kept
    pub environment: String,
    pub skills: Vec<String>,               // sorted
    pub env_policy: String,
    pub env_allow: Vec<String>,            // sorted
}
pub struct NodeFact { pub id: String, pub kind: String, pub interactive: bool,
                      pub profile: Option<ProfileRef>, pub connectors: Vec<String>,
                      pub playbook_ref: Option<String> }
pub struct ProviderRoute { pub name: String, pub base_url: String, pub api_key_ref: String,
                          pub model_overrides: Vec<String> } // not serialized when empty
pub struct ExecutionFacts {
    pub playbook_id: String,
    pub default_profile: Option<ProfileRef>,
    pub nodes: Vec<NodeFact>,                         // sorted by id
    pub profiles: Vec<(ProfileRef, Known<ProfileFact>)>, // sorted by (name, scope)
    pub routes: Known<Vec<ProviderRoute>>,            // sorted by name
    pub decisions_enabled: Known<bool>,
    pub compaction_enabled: Known<bool>,
}
```

Only unordered collections are sorted; fallbacks and tiers keep their
order, since it decides routing.

`Runner` gains `inspect(&PlaybookId)` and `inspect_source(&str)`, both
returning `Result<ExecutionFacts, RunnerError>`. They are added in group 4
together with every implementation: `ApbRunner`, the `RoundRunner`
delegation in `crates/nearbee/src/lib.rs` (`Up` and `Down`), and every
fake runner in the tests, so each group compiles.

`ApbRunner` (`crates/nearbee-apb/src/lib.rs`):

1. Playbook: `GET /api/playbooks/{id}?workspace=<ws>` or the given YAML,
   parsed with `yaml_serde`. A profile reference is either a string
   (scope `auto`) or `{name, scope}` (apb `crates/apb-core/src/profile.rs:63`).
   `interactive` from the node (apb `crates/apb-core/src/schema.rs:2036`).
2. Profiles: every distinct reference from `defaults.profile` and nodes.
   Scope `project` or `global`: `GET /api/profiles/{name}?workspace=<ws>&scope=<scope>`
   only. Scope `auto`: `project`, then `global` on 404. The response's
   `profile_yaml` gives executor, fallbacks, tiers, environment, skills,
   `process_env`. A 404 in every scope, an answer without `profile_yaml`
   or a document that does not parse (a malformed fallback or tier entry
   included: the whole profile, never the entry alone) makes that
   reference `Unknown(<reason>)`. A transport error or a server error
   fails the whole inspection (review round, items 5 and 6): it says
   nothing about the configuration, and a reason that changed from round
   to round would move the fingerprint. The list route is not used.
3. Routes: `<workspace>/opencode.json`, `provider.<name>.options.baseURL`,
   `options.apiKey`, and the model entries under `provider.<name>.models`
   that carry any key beyond display and metadata (`name`, `limit`,
   `cost`, `attachment`, `reasoning`, `temperature`, `tool_call`,
   `modalities`, `release_date`), recorded as `<id>: <keys>`. The routes
   are `Unknown` when the file is missing or unreadable, when the apb
   instance's environment sets any `OPENCODE_` variable (apb keeps them
   under an allowlist, `crates/apb-core/src/agents/opencode/mod.rs:136`),
   and when OpenCode would merge another configuration (OpenCode 2.2.5's
   config discovery, read from the installed release): the unit's
   `XDG_CONFIG_HOME` is not `apb.xdg_config_home` (default
   `~/.local/share/nearbee/xdg-config`) or that directory holds an
   `opencode` entry; the workspace has `.opencode/`; `/etc/opencode`
   exists; a directory above the workspace holds `opencode.json`,
   `opencode.jsonc` or a `.opencode/` with more than OpenCode's install
   files (`bin`, `node_modules`, `package.json`, `package-lock.json`,
   `bun.lock`, `.gitignore`). The supported launch is the unit
   `scripts/apb-instance.sh` writes, which sets no `OPENCODE_` variable and
   sets `XDG_CONFIG_HOME` to Nearbee's empty directory; the runner reads
   the instance environment from
   `systemctl --user show nearbee-apb -p Environment` (`SYSTEMCTL_BIN`
   names another binary) through the existing CLI helper, and a failed
   read fails the inspection. Not covered: OpenCode also loads
   instructions and skills from `~/.claude` and `~/.agents` (and from
   `.claude/` and `.agents/` above the workspace) whatever
   `XDG_CONFIG_HOME` says; see `docs/privacy.md`.
4. Decisions: `<workspace>/.apb/config.yaml`, `decisions.enabled`;
   `false` gives `Yes(false)`, `true` gives `Yes(true)`, absent gives
   `Unknown`. Nearbee's apb project sets `decisions: { enabled: false }`
   (written by `scripts/apb-instance.sh install`). Consequence, documented
   in `docs/privacy.md`: the intake scope check goes unanswered and is
   skipped for every listing while it is off (existing behaviour of
   `crates/nearbee-core/src/intake.rs`).
5. Compaction: apb `v1.0.0-beta.6` compacts only when a run's
   `context_max_bytes` is set, and no REST, CLI or MCP surface sets it
   (`crates/apb-engine/src/scheduler/prepare.rs:1124` is the only writer).
   `compaction_enabled` is `Yes(false)` when the instance reports version
   `1.0.0-beta.6`, `Unknown("apb version <v>")` otherwise; a failed health
   read fails the inspection.
6. Fixtures: the integrator records sanitized live responses of the
   playbook detail, a project and a global profile detail, and the health
   route from the instance on port 7322 into
   `crates/nearbee-apb/tests/fixtures/apb/` before group 4 (task 3.4);
   tests replay them; synthetic fixtures only for errors and edge cases.

### 3. The tier check

`tiers::check(tier, &facts, &catalog) -> Result<(), Vec<String>>`, pure,
every breach one English line naming the node, profile or model. Rules as
spec `privacy-tiers`: every executor, fallback and tier of an attested
profile must run the agent `opencode`; the `nearai` route must read its
key as `{env:NEARAI_API_KEY}` and declare no model override; an unknown
decisions or compaction setting refuses every tier. Known node kinds: `start`, `finish`, `agent_task`,
`script`, `prompt`; any other kind is a breach (`human_review` named).
The catalog is `docs/reference/near-ai-cloud-models.json` embedded with
`include_str!` in `crates/nearbee-core/src/catalog.rs` (`models[].id`,
`models[].owned_by`); a `nearai/<id>` model strips `nearai/` before lookup.
`tiers::evidence_models(&facts)` returns the sorted distinct stripped ids
of every usable profile's executor, fallbacks and tiers.

### 4. Statement, fingerprint, drift

`crates/nearbee-core/src/statement.rs`:

1. `render(tier, &facts, retention_days) -> String`, `STATEMENT_FORMAT = 1`,
   sections in this order under `## Data handling`: "Models" (executors in
   order: `claude` is "the owner's Claude subscription (Anthropic)",
   `codex` "the owner's OpenAI subscription", `opencode` on a checked
   `nearai/` model "NEAR AI Cloud Confidential TEE model `<id>`", any
   other "agent `<agent>`, model `<model>`"), "Other channels"
   (connectors by name, decisions on or off, compaction on or off, an
   unknown setting as "unknown" without its reason, which may name a path;
   when any is on: "these channels may send data to providers Nearbee has
   not validated"), "Where the run happens" (the owner's machine can read
   the data; the agent steps run without a sandbox, so reading the owner's
   files and undeclared network access are not prevented), "What the
   market keeps", "Local retention".
2. `fingerprint(tier, &facts) -> String`: `sha256:` + hex SHA-256 of
   `serde_json::to_vec(&(tier, facts))`.
3. `drift(record, &facts, retention_days) -> Option<String>`: `None` when
   the record is serveable (decision 1.3), the check passes, the
   fingerprint matches and `render` equals the recorded text; otherwise the
   reason.
4. `nearbee list` (`crates/nearbee/src/publish.rs`): parse; materialise;
   `inspect_source`; check; install; `inspect`; check; equal fingerprints
   (else "installed facts differ from the source"); render; write the
   record with `evidence_models`; then the market reads and the report,
   which prints `tier:` and the statement block. Any failure before the
   record write leaves the old record.
5. Serve: at the start of every round (before pass 2b), for every listing,
   `inspect` + `drift`; the verdict (`ListingVerdict`: `Clean`,
   `Drifted(reason)`, or `Held(error)` when `inspect` failed) is kept in
   `RoundState` for that round only. A drift is printed once per change:
   the handle is remembered and forgotten again on a clean round, so a
   later drift is printed again. `intake_short` and `admit` consult it for
   a hire whose current generation has no `run_start` intent: drifted
   declines at once by `waiting_decline` with reason and attention
   `listing_drift`; held runs no intake check, scope check, market start
   or apb start and declines nothing, so the advance pass's
   waiting-past-deadline rule declines the hire in the end.
6. `waiting_decline` gains `reason: &str`, `buyer: &str`, `attention: &str`
   parameters; its existing callers pass today's texts. The
   rejected-or-failed choice stays inside it.

### 5. Start preflight: statement snapshot

1. For every generation, before the market start of a first-time hire and
   before the apb start of any generation: when the generation has no
   `statement.json` snapshot, write it with `journal::write_snapshot`
   (flat name `statement.json`):
   `{"tier","statement","statement_sha256","statement_format","fingerprint","evidence_models"}`
   from the current record. A record that is not serveable declines with
   `listing_drift` instead (it would freeze no statement and the
   generation would deliver as legacy). When it exists and the generation has no
   `run_start` intent yet, reuse it only when its fingerprint and statement
   hash equal the record's; otherwise decline with `listing_drift`.
2. The `run_start` intent refs gain `tier`, `statement_sha256`,
   `fingerprint`.
3. Delivery reads the snapshot. A `run_start` intent without
   `statement_sha256` is legacy (S3): delivered without footer or
   evidence. With it, a missing snapshot or a hash mismatch declines
   `failed` with attention `statement_missing`.
4. `validate_deliverable` assembles
   `markdown + "\n\n---\n\n" + evidence section (attested) + "\n\n" + statement`,
   then runs `manifest::check_final_markdown` and `scan_secrets` on the
   assembled text; the submission snapshot stores the assembled text, so
   recovery resends it as is.

### 6. Endpoint evidence

1. Seam: `crates/nearbee-core/src/evidence.rs` defines
   `trait Attestor: Send + Sync { fn report(&self, model: &str, nonce: &str) -> impl Future<Output = Result<Vec<u8>, String>> + Send; }`,
   the snapshot record type, the journal payloads and the section
   renderer. `crates/nearbee-cloud` implements it: `CloudAttestor::new(base_url, key: nearbee_core::secret::Secret)`;
   production passes `https://cloud-api.near.ai/v1` and
   `Config.nearai_api_key`; `nearbee serve` builds it only when an attested
   listing is served, and a missing key then makes every fetch fail with
   "NEARAI_API_KEY is not set". Request: `GET {base}/attestation/report`
   with query `model`, `signing_algo=ecdsa`, `nonce`; header
   `Authorization: Bearer <key>`; `redirect::Policy::none()`; per attempt
   timeout 9 s; 2 attempts with a 1 s wait on timeout, connect error or
   5xx; body read in chunks to 2 MiB; 2xx with a JSON object is success.
2. Journal events (new `Event` variants, round-trip tested):
   `EvidenceCollected { phase: "start"|"end", model, sha256 }` and
   `EvidenceFailed { phase, model, reason }` (reason redacted and at most
   200 characters), both carrying the generation like every record.
3. Snapshot files: flat names
   `evidence-<phase>-<first 16 hex of sha256(model)>.json` through
   `journal::write_snapshot`, content
   `{"model","phase","nonce","fetched_at","sha256","body_b64"}` where
   `sha256` is of the decoded body. Reuse checks the decoded body's hash.
4. Scheduling: a hire needing a report is a long-step candidate in pass 3
   with the same oldest-first order and cut-off as downloads; one fetch per
   round; the fetch writes the snapshot file, then the
   `EvidenceCollected` record. A file without its record is completed on
   the next round by writing the record (no fetch).
5. Start: an attested generation whose frozen `evidence_models` lack a
   start report waits (no market start, no apb start) until all are
   collected; start reports are fetched only while the generation has no
   `run_start` intent. After a `run_start` intent, a missing or mismatched
   start report declines `failed` with attention `evidence_missing`.
6. End: after a succeeded run's manifest validates, delivery waits until
   every end report is collected, then assembles.
7. Failures: on the third `EvidenceFailed` record of a phase in the
   generation, decline at once by `waiting_decline` with attention
   `evidence_unavailable` (start) or `failed` with buyer reason
   "the run evidence could not be collected" (end).
8. Renderer: `evidence::section(models, start, end, attempts)` builds the
   Markdown of spec `run-evidence`, Disclosure; the link URL-encodes the id
   (`/` as `%2F`).

### 7. Attempt models

`RunDetail` (`crates/nearbee-core/src/runner.rs`) gains
`attempts: Vec<AttemptModel { node, attempt, agent, requested_model: Option<String>, reported: Reported }>`
with `enum Reported { None, One(String), Mixed }`. `crates/nearbee-apb`
parses apb's `attempt_models` (`reported.kind`: `none` gives `None`; a
single reported model gives `One`; anything else `Mixed`; shape from apb
`crates/apb-engine/src/execution_identity/fold.rs:426`). Every
initializer of `RunDetail` is updated in the same group. The terminal
`run_observed` journal record gains an optional `attempts` field (absent
reads as empty), written by every path that journals a terminal
observation (`crates/nearbee-core/src/serve.rs` near lines 2499 and
2651); delivery reads it from the journal.

### 8. Agent A tools

1. Scanner: `crates/nearbee-core/src/intake.rs` gains
   `github_repos(brief) -> Vec<(String, String)>` per spec `repo-audit`,
   Repository URL scan (hand-written, no regex crate). `intake_short` runs
   it for listings with `intake_check: github_repo` before the scope
   check and declines `rejected` with the spec's buyer text.
2. Run params: `crates/nearbee-apb/src/run_params.rs`,
   `pub fn from_env() -> Result<BTreeMap<String, String>, String>` reading
   `$APB_RUN_DIR/run.yaml` (`params`, apb
   `crates/apb-engine/src/run_config.rs:146`), with a fixture test.
3. Commands `nearbee audit fetch` and `nearbee audit finish` in
   `crates/nearbee/src/audit.rs`, run in the current directory (the run's
   worktree). They are `async` on the binary's existing tokio runtime;
   archive decoding runs in `tokio::task::spawn_blocking` with the
   deadline checked per member. `fetch_snapshot(api_base, codeload_base, brief, dir)`
   and `finish(dir)` are the testable functions; the subcommands pass the
   production bases `https://api.github.com` and
   `https://codeload.github.com`.
4. HTTP: `reqwest` with user agent `nearbee-audit`,
   `redirect::Policy::none()`, 20 s per request, a 120 s overall deadline.
5. Archive: `flate2::read::GzDecoder` over a reader capped at 100 MiB
   compressed, wrapped by a counting reader that fails past 100 MiB
   decompressed, into `tar::Archive::entries()`; after the last entry the
   rest of the gzip stream is read to its end through the same counting
   reader, so a missing trailer or a wrong CRC fails. Staging directory
   `audit-snapshot.partial/` created fresh (an existing one is removed
   first, never followed if it is a link); files written with
   `create_new`; parents created only inside staging; duplicate and
   colliding paths fail. The tar top directory is stripped.
   `source.json` (each file with its hex SHA-256) and `coverage.json` are
   written into staging, every file
   and directory is synced, then staging is renamed to `audit-snapshot/`
   and the worktree directory synced.
6. Finish: inputs opened with no-follow per component (reuse the
   descriptor approach of `crates/nearbee-core/src/manifest.rs`), capped at
   2 MiB; `serde` with `deny_unknown_fields`; `out/result.json` removed
   first; a finding whose file no longer has the SHA-256 `source.json`
   records is rejected `file changed after fetch` before its lines are
   read; outputs written to temporary names and renamed; `result.json`
   last, with the real size and SHA-256 of `deliverable.md`.
7. Escaping: finding text has `\`, `` ` ``, `*`, `_`, `[`, `]`, `#`, `|`,
   `<`, `>` and `&` escaped (`<`, `>`, `&` as entities, the rest by
   backslash); line starts with `-`, `+`, `<digits>.` or a run of three or
   more `~` are escaped. The
   JSON fence uses `max(3, longest backtick run + 1)` backticks.

### 9. The playbook

`playbooks/repo-audit/playbook.yaml`, id `repo-audit`, version 1.0.0,
`defaults.profile: nearbee-own` (each listing overrides it), params as the
S2 playbook (`brief`, `feedback`, `inputs`, `hire_id`, `execution_id`),
nodes:

1. `fetch` (script `scripts/fetch.sh`: `exec "${NEARBEE_BIN:-nearbee}" audit fetch`);
2. `audit` (agent_task, not interactive): reads
   `audit-snapshot/source.json`, `audit-snapshot/coverage.json` and files
   under `audit-snapshot/repo/`; writes only `audit/candidates.json` in the
   schema spelled out in the prompt; never runs programs from the
   snapshot; on a non-empty feedback revises the previous candidates;
   `accept_untrusted` set; `writes: ["audit/**"]`, `max_changed_files: 1`
   and `protect: ["audit-snapshot/**"]`, the strongest restrictions apb
   1.0.0-beta.6 enforces for it (apb `crates/apb-core/src/schema.rs`).
   No `isolation`: opencode declares no sandbox needs at the tag, so
   `full` refuses (`agent_needs_unknown`) and `best_effort` runs without a
   sandbox in a scratch directory that lacks the snapshot. `protect` does
   not cover git-ignored paths (a `.gitignore` inside the fetched
   repository), which the per-file SHA-256 check in `finish` covers;
3. `finish` (script `scripts/finish.sh`: `exec "${NEARBEE_BIN:-nearbee}" audit finish`);
4. `done`.

`scripts/apb-instance.sh` sets `NEARBEE_BIN` and `XDG_CONFIG_HOME` (Nearbee's
own empty directory) in the unit environment.
Listings: `listing.repo-audit-fast.yaml` (tier `attested`, profile
`nearbee-near`, price "0.50") and `listing.repo-audit-premium.yaml` (tier
`plain`, profile `nearbee-own-premium`, price "2.00"); both SLA 86400,
category `dev_tools`, `intake_check: github_repo`, scope
"An audit of one public GitHub repository named by its https://github.com URL in the brief.".

### 10. The signed step contract (for S5)

Written into `docs/privacy.md`, "Attested, signed": `nearbee signed-call
--request <file> --out <dir>` sends the request file's exact bytes with
`stream: false` to `POST /v1/chat/completions`, stores the response bytes
before parsing, fetches
`GET /v1/signature/{completion id}?model={id}&signing_algo=ecdsa`,
requires `signature_kind: "provider_tee"` and
`text == model + ":" + sha256(request) + ":" + sha256(response)`, retries
only the signature GET, never the completion, journals its own operation
state so a node retry resumes the signature fetch, bounds tokens and wall
clock, and writes manifest evidence entries (`kind: tee_signature`,
private bytes, public hashes and signature). Verifying the signature is
not part of it.

## Risks / Trade-offs

1. [The implementer cannot build] → the integrator builds, tests and runs
   clippy after every group and hands back failures in a fixes file.
2. [apb route shapes differ from the source read] → replay tests run on
   responses recorded from the live instance (decision 2.6).
3. [Decisions off for the whole project] → the scope check is skipped for
   every listing; `intake_check: github_repo` is the deterministic
   replacement for agent A; documented.
4. [GitHub API limit of 60 requests per hour without a token] → two per
   hire; enough for the demo; documented.
7. [The agent step runs unsandboxed on attacker-chosen content] → apb
   enforces the write scope and the snapshot protection, `finish` checks
   every cited file's hash, and the statement and `docs/privacy.md` say
   that reading the owner's files and the network are not prevented.
8. [An apb hiccup holds a listing] → a held round starts nothing and
   declines nothing; a hire held past its deadline is declined by the
   waiting rule as before.
5. [Endpoint reports are not verified] → labelled; Verification Center
   links let the buyer verify.
6. [Evidence adds rounds to an attested hire] → one fetch per round with
   two models adds about four rounds (three minutes at the 45 s interval);
   inside a 24 h SLA.

## Migration Plan

1. `playbooks/first-hire/` is deleted; tests and docs move to
   `playbooks/repo-audit/`.
2. The S3 record of `repo-audit-fast` is drifted until listed again from
   `playbooks/repo-audit/listing.repo-audit-fast.yaml`.
3. S3 generations (no `statement_sha256` in their `run_start` intent)
   deliver as in S3.
