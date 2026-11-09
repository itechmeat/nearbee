# Design: foundation and runner

## Context

See `proposal.md` (Why) and `brainstorm.md` (approach and evidence). apb
facts below were read from the apb source on `origin/main` by the brainstorm
partner and must be confirmed by the fixture capture against the installed
apb (1.0.0-beta.6 at the time of writing); where they differ, the installed
apb wins and `docs/apb.md` records it.

## Goals / Non-Goals

Goals: types and seams later stages build on without migration; a runner
that cannot start a duplicate run by itself; a manifest validator that cannot
be tricked into delivering bytes it did not check; two recorded live runs on
NEAR AI Cloud.

Non-goals: a journal writer, hire orchestration, any market call, intake,
pricing, concurrency beyond one run at a time.

## Decisions

### 1. Workspace layout

```
Cargo.toml                 workspace, edition 2024, resolver 3, [workspace.dependencies]
crates/nearbee-core        domain, traits, manifest, journal types, config
crates/nearbee-apb         ApbRunner (reqwest), fixtures under tests/fixtures/apb/
crates/nearbee-market      empty lib with a module doc pointing to S2
crates/nearbee             binary `nearbee` (clap)
playbooks/profiles/        nearbee-own.yaml, nearbee-near.yaml, nearbee-near-vision.md
playbooks/smoke/           smoke.yaml (synthetic, no buyer data)
.github/workflows/ci.yml   fmt, clippy -D warnings, test, openspec validate --strict
```

`rust-toolchain.toml` pins the current stable channel. Lints:
`unsafe_code = "forbid"` at the workspace level.

### 2. Traits in core

```rust
pub trait Runner: Send + Sync {
    fn workspace(&self) -> &WorkspaceId;
    fn playbooks(&self) -> impl Future<Output = Result<Vec<PlaybookSummary>, RunnerError>> + Send;
    fn estimate(&self, id: &PlaybookId) -> impl Future<Output = Result<Estimate, RunnerError>> + Send;
    fn start(&self, req: StartRequest) -> impl Future<Output = Result<RunId, RunnerError>> + Send;
    fn run(&self, id: &RunId) -> impl Future<Output = Result<RunDetail, RunnerError>> + Send;
    fn find(&self, q: RunQuery) -> impl Future<Output = Result<RunLookup, RunnerError>> + Send;
}
pub trait Market: Send + Sync {}   // methods arrive in S2
```

`StartRequest { playbook, instruction, params: BTreeMap<String,String>,
worktree: PathBuf, hire_id: HireId, execution_id: ExecutionId, budget:
Budget { usd: Option<Decimal>, on_no_estimate: OnNoEstimate } }`. Callers
cannot omit `hire_id` or `execution_id`; the runner inserts them into
`params` and refuses a caller param of the same name. `RunLookup { runs,
complete: bool }`. `RunStatus` is an enum with `Unknown(String)`.

`RunnerError` (thiserror): `Refused { status, code, detail }`,
`RetryableRead { detail }`, `StartOutcomeUnknown { detail }`,
`Protocol { detail }`, `Invalid { detail }` (caller error caught before
sending). `detail` is cut to 2 KiB and passed through a redactor that removes
the configured secret values (longest first) and `Bearer` tokens, quoted or
not. A refusal's `code` is redacted and cut to 128 bytes; every string in its
`estimate` is redacted and an estimate over 8 KiB serialized is dropped. A
start answered with a 3xx (redirects are not followed) or a transport error
is `StartOutcomeUnknown`.

Money is `rust_decimal::Decimal` (add the crate), never `f64`. apb's
budget takes `usd` as a JSON float, so the conversion happens only at that
wire boundary; ceiling arithmetic stays in `Decimal` on Nearbee's side.

A 200 answer with a usable run id (one URL path segment, the same check every
later read applies) is always a known start, also when apb queued it behind a
busy workdir; a 200 without one, or with an empty or unusable id, is
`StartOutcomeUnknown`, so the caller recovers the run by its ids. Launch-gate and irreversible-confirmation refusals are
`Refused` and are never answered by the runner.

### 3. HTTP client

One `reqwest::Client` with rustls, `retry(retry::never())` (reqwest 0.13's
default policy resends a request, body included, on an HTTP/2
`REFUSED_STREAM` or graceful `GOAWAY`; fix round 3), `redirect(Policy::none())`,
`no_proxy()`, connect timeout 3 s. Per call: reads 10 s, estimate 30 s, start 30 s.
Reads (`GET`) retry up to 3 attempts with 0.5 s, 1 s, 2 s backoff on
transport errors and 429, 502, 503, 504; a start is never retried. Base URL
from `APB_URL` (default `http://127.0.0.1:7322`, Nearbee's own apb
instance, decision 10). Endpoints:

| Operation | Request |
| --- | --- |
| workspace | `GET /api/projects`, match the canonical configured path |
| playbooks | `GET /api/playbooks`, filter by workspace locally |
| estimate | `GET /api/playbooks/{id}/estimate?workspace=` |
| start | `POST /api/playbooks/{id}/run?workspace=` body `{instruction, params, worktree, budget}` |
| run | `GET /api/runs/{id}?workspace=` |
| find | `GET /api/runs?workspace=&playbook=&field=<name>:<value>` -> `{runs, not_indexed, withheld}`; `complete = not_indexed == 0 && withheld == 0` |

Exact field names are taken from the captured fixtures, not from this table.

### 4. Fixtures

`crates/nearbee-apb/tests/fixtures/apb/<name>.json` holds `{captured_at,
apb_version, request: {method, path, query, body}, response: {status,
headers, body}}`. A capture is done by an explicit example binary
`cargo run -p nearbee-apb --example capture -- <name>` against the live
dashboard; tests never write fixtures. Captured bodies are scrubbed (keys,
home paths replaced by `<home>`). Tests replay them with `wiremock`, matching
method, path, query and body, so a wrong query or body fails the test.

### 5. Manifest v1

Schema in `nearbee-core::manifest` with `#[serde(deny_unknown_fields)]`.
Defaults (configurable under `limits:`): Markdown 262,144 bytes, per file
25 MiB, at most 20 files, total 100 MiB. Algorithm: a process the run left
behind may still change the worktree while it is validated, so nothing is
resolved by path twice. Open the canonical worktree, then `out` relative to it
with `openat(O_DIRECTORY | O_NOFOLLOW)`, once; for each entry reject absolute
paths and any component other than a normal name; resolve each parent from
the previous directory descriptor with `openat(O_DIRECTORY | O_NOFOLLOW)` and
the leaf with `openat(O_NOFOLLOW | O_NONBLOCK | O_NOCTTY)` (a link anywhere on
the path fails its own open; a FIFO opens at once instead of blocking); the
opened leaf must be a regular file (`fstat`); read with `take(limit + 1)`,
compare length and SHA-256; keep the bytes in `ValidatedManifest`. A final
`check_final_markdown(&str)` enforces the limit on the assembled text. The
safe `openat` comes from `rustix` (the workspace forbids `unsafe`); off unix
validation fails closed (security review r1, final review fix round 3).

### 6. Journal types

`nearbee-core::journal`: `Record { schema_version: 1, seq, timestamp,
hire_id, event_type, payload }` with `event_type` an enum over `Received`,
`Decision`, `Intent`, `Result`, `RunObserved`, `ArtifactInventory`,
`Deleted`, and `Unknown { raw }`. `Intent { operation_id: Uuid (v7), kind,
execution: u32, request_digest, refs }`, `Result { operation_id, outcome,
external_ids }`. A pure reader turns a list of lines into records and open
intents; a trailing incomplete line is reported as truncated, a malformed
interior line is an error. `safe_id(hire_id)`: keep `[A-Za-z0-9_-]`, replace
the rest with `_`, keep the first 64 characters, append `-` and the first 16
hex of the id's SHA-256 so two ids never collide and the name stays far below
the 255-byte file name limit. No file writing in S1.

### 7. Config

`nearbee.example.yaml`:

```yaml
state_dir: .nearbee
apb:
  url: http://127.0.0.1:7322
  workspace_path: ~/.local/share/nearbee/apb-workspace
limits:
  markdown_bytes: 262144
  file_bytes: 26214400
  files: 20
  total_bytes: 104857600
```

Variables: `APB_URL`, `APB_API_KEY` (optional), `NEARAI_API_KEY` (required
for the near profile), later stages add their own. `Secret` wraps a
`String`, implements `Debug`/`Display` as `"<redacted>"`, exposes the value
only through `expose()`. `.env` is read with `dotenvy` without overriding
existing process variables. The apb URL is parsed at load with the `url`
crate (the parser reqwest uses, so both agree on the authority); a value
that is not an absolute `http` or `https` URL, or that carries userinfo
(`user:key@`), is refused, naming its source and never its value. `config check`
prints `ok`/`missing`/`failed` per line; every line the CLI prints passes
through the secret redactor.

### 8. Cloud path

OpenCode custom provider in the dedicated apb workspace's `opencode.json`:
provider `nearai`, `npm: @ai-sdk/openai-compatible`, `baseURL
https://cloud-api.near.ai/v1`, `apiKey: {env:NEARAI_API_KEY}`, models
`Qwen/Qwen3.8-27B` and `deepseek-ai/DeepSeek-V4.1-Flash-1017`. Profile
`nearbee-near`: `executor: {agent: opencode, model: nearai/Qwen/Qwen3.8-27B}`,
fallback `nearai/deepseek-ai/DeepSeek-V4.1-Flash-1017`, `process_env`
allowing `NEARAI_API_KEY`. Nearbee's apb instance (decision 10) is started
with `NEARAI_API_KEY` in its environment (executors inherit apb's
environment, not Nearbee's `.env`). When apb ships
its `near-ai-cloud-models` change, the profile switches to `agent: dsh` and
nothing else changes.

The smoke playbook: one `agent_task` node on the profile, instruction
"write a three-line Markdown note about the given topic into
out/deliverable.md", then one trusted `script` node that writes
`out/result.json` with the real size and SHA-256. Params: `topic`,
`hire_id` (`search: true`), `execution_id` (`search: true`). It is run on
each model by a profile override or a materialised copy (whichever the
installed apb supports over REST; the brainstorm found no `overrides` on the
REST start, so two copies `smoke-qwen` and `smoke-deepseek`).

### 9. Live runs

Two runs, one per model, with `on_no_estimate: continue` and `usd: 0.50`.
Recorded in `docs/apb.md` and `docs/near-ai-cloud.md`: model, apb and
OpenCode versions, run id, terminal status, manifest hashes, usage and cost
as reported (or "not reported"). The live test is an `#[ignore]` test run
with `cargo test -p nearbee-apb -- --ignored live_` and the
`NEARBEE_LIVE=1` variable; CI never runs it.

### 10. Nearbee's own apb instance

Nearbee never uses the owner's everyday apb dashboard (port 7321, config in
`~/.config/apb`). It runs its own: `APB_CONFIG_DIR=~/.local/share/nearbee/apb-config
apb dashboard --no-open --port 7322`, started by `scripts/apb-instance.sh
start|stop|status` as a transient `systemd --user` unit `nearbee-apb` with
`.env` as its environment file (so `NEARAI_API_KEY` reaches the executors and
is never printed). Reasons: the owner's dashboard, profiles and learning data
are untouched; the instance's config, memory and run data can be wiped with
the hires (D-012, D-023); the key lives only in that instance's environment.
The dedicated workspace and the profiles are registered in this instance
only. On machines without systemd the script prints the equivalent command.

## Risks / Trade-offs

1. [OpenCode does not pass reasoning output of DeepSeek cleanly] -> the
   Qwen run is the acceptance minimum; a DeepSeek failure is recorded with
   its error and reported, not hidden.
2. [Credits are not topped up when the live task is reached] -> the
   implementer stops at that task and the integrator asks the owner; the
   stage is not called done without the two runs.
3. [Installed apb differs from `origin/main` field names] -> fixtures are
   captured from the installed apb; the code follows them.
4. [`not_indexed` counts runs created before apb's search index existed,
   so a lookup in a workspace with old runs is never complete] -> the
   dedicated workspace is new and holds only Nearbee's runs; the fixture
   capture records the counts there; if they are not zero, `docs/apb.md`
   says so and S3 treats lookups as incomplete.
5. [Time-of-check window in manifest validation] -> accepted for a finished
   run's worktree; the validated bytes, not the paths, go forward.
6. [D-012 cannot be enforced by apb today] -> S1 uses synthetic data only;
   apb's external-data flag is requested (D-023).

## Migration Plan

None: first change.

## Open Questions

None that change specs or tasks.
