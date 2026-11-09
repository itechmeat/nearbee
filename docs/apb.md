# Agentic Playbooks (`apb`): the surface Nearbee drives

Source: https://github.com/itechmeat/agentic-playbooks (public, Apache-2.0).
Version at the time of writing: `apb 1.0.0-beta.6` (installed; Nearbee reads apb's source at that tag). Its `docs/` folder is the
authority; the facts below were checked there on 2026-11-09 (`DEPLOYMENT.md`,
`NOTIFY.md`, `CONNECTORS.md`, `PROFILES.md`, `DECISIONS.md`,
`HOST-INTEGRATION.md`, `crates/apb-server/src/routes/*`).

## What apb is, in Nearbee's terms

A local engine that runs YAML playbooks: graphs of `agent_task`, `script`,
`condition`, `human_review`, `wait` nodes with parallel branches, retries,
fallbacks, budgets and an append-only journal. Agent steps are executed by the
coding-agent CLI a **profile** names (claude, codex, opencode, cursor, qoder,
hermes, grok, zcode, pi, antigravity; DSH native in a coming release), with
ordered fallbacks and optional tiers (light, standard, heavy). The same binary
serves a dashboard and REST API (`apb dashboard`), an MCP server (`apb mcp`) and
notifications through connectors (`apb notify`).

Nearbee talks to apb over the REST API of a running `apb dashboard` (server
mode), never by embedding apb. That keeps Nearbee independent of apb's language
and release cycle.

## Server mode

- `apb dashboard --no-open` binds loopback by default; a non-loopback bind
  requires API keys and a reverse proxy (`docs/DEPLOYMENT.md`).
- Auth: with at least one key issued (`apb server key issue`, as the user the
  dashboard runs as), every request needs `Authorization: Bearer apb_...`.
  Without keys (the loopback default) the dashboard authenticates nobody and
  only guards against browsers: it answers only its own host names and wants
  `x-requested-with: apb-dashboard` or `Sec-Fetch-Site: same-origin` on a
  browser's write. That header is a CSRF marker, not a credential: a client
  that is not a browser (no `Origin`, no `Sec-Fetch-Site`), such as Nearbee,
  needs neither. Verified 2026-11-09: Nearbee's keyless loopback instance
  answers all of Nearbee's calls, writes included, with no credential.
- Nearbee sends the bearer header only when `APB_API_KEY` is set, follows no
  redirects and uses no system proxy.
- One dashboard serves one config dir (`<config_dir>/serve.lock`).

## Nearbee's own apb instance

Nearbee never uses the operator's everyday dashboard (port 7321, config in
`~/.config/apb`). It runs its own, so the operator's profiles, registry and
learning data stay untouched and Nearbee's run data can be wiped with the
hires (D-012, D-023). `scripts/apb-instance.sh`:

| Command | What it does |
| --- | --- |
| `start` | `APB_CONFIG_DIR=~/.local/share/nearbee/apb-config apb dashboard --no-open --port 7322` as the transient `systemd --user` unit `nearbee-apb` (`KillMode=process`, so stopping it leaves run drivers alone); prints the command instead on a machine without a systemd user session |
| `stop`, `status` | stop the unit and remove the key file (and any temp key file a killed `start` left); unit state and `/api/health` |
| `register <dir> --git` | `git init` plus an empty commit, `apb init --standing-block no --feedback-loop no`, and one registering command, all with this instance's `APB_CONFIG_DIR`: the directory becomes a project of this instance only |
| `install [<workspace>]` | copies `playbooks/profiles/*.yaml` as project profiles (`.apb/profiles/<name>/profile.yaml` plus a one-line `SOUL.md`), `playbooks/workspace/opencode.json` to the workspace root, and imports each `playbooks/smoke/*.yaml` with its `scripts/` as a bundle (`apb import --accept <bundle digest>`), then approves the version, also an unchanged one, by its trust digest (`apb trust approve <id> --version <v> --digest <d>`): a version imported with scripts does not run until approved. Before and after the approval, `apb import <bundle> --preview` must say the current version is `<v>` and already holds the bundle just built (apb compares definition and scripts); otherwise nothing is approved, or the approval just made is revoked. A version that exists with other content (apb: "already exists") stops the install with "bump the version". Prints `playbook <id> <v> approved` per playbook and exits non-zero naming the playbook when apb prints no digest or refuses it |
| `allow-billed` | writes `billing: {billed: allow, unknown_basis: allow}` to the instance's `policy.yaml`; see "Billed work" below. Only with the owner's go-ahead for Cloud spend |
| `apb <args>` | the apb CLI against this instance's config dir |

Setup from scratch (2026-11-09):

```sh
scripts/apb-instance.sh start
scripts/apb-instance.sh register ~/.local/share/nearbee/apb-workspace --git
echo work/ > ~/.local/share/nearbee/apb-workspace/.gitignore
scripts/apb-instance.sh install
curl -s http://127.0.0.1:7322/api/projects     # lists the workspace
```

The provider key: executors inherit apb's environment, not Nearbee's `.env`.
`start` copies only the variables named in `NEARBEE_APB_ENV_VARS` (default
`NEARAI_API_KEY`) from `.env`, read the way Nearbee reads it
(`scripts/dotenv-export.py`, a port of dotenvy 0.15.7: inline comments,
quotes, a whitespace-prefixed `export`, the last assignment wins, an empty
value is not set; a file Nearbee could not parse is refused), written as
`NAME="value"` (the form systemd and the shell read alike) into a fresh 0600 file under `$XDG_RUNTIME_DIR`
(written as a temporary file and moved over the old one, so an existing file's
wider mode is never kept; a failed or interrupted write removes the temporary
file) and passes it as the unit's `EnvironmentFile`; values are never printed. Other
`.env` entries (market tokens) never reach apb or an executor. Profiles with
`process_env: allowlist` then pass `NEARAI_API_KEY` to the OpenCode executor
by name (`allow: [NEARAI_API_KEY]`), since apb does not know the custom
`nearai/` provider's key on its own (`PROFILES.md`, process environment).

A gotcha: any registering apb command run with this `APB_CONFIG_DIR` inside a
directory that has `.apb/` registers that directory in Nearbee's instance (an
`apb detect` from an apb source checkout did, 2026-11-09). Run instance
commands from the workspace or a neutral directory; remove a stray entry with
`scripts/apb-instance.sh apb projects remove <workspace_id>`.

## REST endpoints Nearbee uses

Every call that accepts one carries `?workspace=<workspace_id>`: the runner
resolves the configured `apb.workspace_path` to its id through
`GET /api/projects` once at start-up and pins it. The playbook list spans
every registered project, so the runner filters it by `workspace_id` itself;
the same playbook id in another project is ignored. Field names below are
from fixtures captured from apb 1.0.0-beta.6 on 2026-11-09
(`crates/nearbee-apb/tests/fixtures/apb/`).

| Purpose | Endpoint | Notes |
| --- | --- | --- |
| Projects | `GET /api/projects` | `[{workspace_id, name, path, playbook_count, parent_workspace_id}]`; `path` is canonical |
| List playbooks | `GET /api/playbooks` | all projects: `[{id, name, current, versions, profiles, project, workspace_id, ...}]` |
| Read one | `GET /api/playbooks/{id}?workspace=` | params (`name`, `type`, `label`, `default`, `secret`, `search`), goal, nodes |
| Estimate a run | `GET /api/playbooks/{id}/estimate?workspace=` | `{cost_usd, tokens, time: {basis, spread: {p50, low, high}}, comparable: {runs, ...}, unestimated, summary}`; `cost_usd` and `tokens` are `null` without history (unknown, not zero); time in ms from declared `expected_duration` until runs exist |
| Fit check | `GET /api/playbooks/{id}/fit` | to inspect; may serve intake |
| Start a run | `POST /api/playbooks/{id}/run?workspace=` | body `{instruction?, params{k: string}, worktree?, budget{tokens?, usd?, on_no_estimate?}, confirm_irreversible?, confirm_launch?, continued_from?, remember?, notify?}`; **no `version` and no `overrides`**: the current version runs unless the playbook has an eligible candidate the trial policy admits (see "Current version and candidates" below). 200 `{run_id, estimate, budget, warnings, candidate_trial?, candidate_skipped?}`; there is no caller request id. Refusals below |
| Follow a run | `GET /api/runs/{id}?workspace=` | see "Run detail" below; 404 with a plain-text body for an unknown run |
| Find runs | `GET /api/runs?workspace=&playbook=&field=<name>:<value>` | `{runs: [{run_id, playbook, status, search: {state, fields}, ...}], not_indexed, withheld}` (at most 8 `field` terms, equality only, text compared without case) |
| Live events | `GET /api/ws` | websocket stream the dashboard uses; polling `GET /api/runs/{id}` is enough for v1 |
| Answer a question | `POST /api/runs/{id}/answer` | `{node?, answer, request?}`; posted as `answered_by: human` |
| Decide a review gate | `POST /api/runs/{id}/review` | `{node, decision, note?, nonce?, request?}` |
| Pause, resume | `POST /api/runs/{id}/pause`, `/resume` | |
| Report | `GET /api/runs/{id}/report` | the run report (also as a page); never a deliverable |
| Notify | `POST /api/runs/{id}/notify` | send the run's notification once |
| Profiles | `GET /api/profiles`, `GET /api/profiles/{name}` | executor agent, model, fallbacks, tiers, environment |
| Models | `GET /api/models` | known models per agent (`model_check`) |
| Stats | `GET /api/stats`, `apb stats --json` | per playbook version and node: outcome rate, retries, **median** durations and tokens, cost; estimates give medians and p25 to p75 ranges, never a p90, and cost is reported only where the executor reported usage |
| Connectors | `GET /api/connectors`, `/api/connectors/{name}` | which connectors a playbook can reach: the subprocessor list |

### Current version and candidates

A start without a version does not always run `current` (apb 1.0.0-beta.6
source: `crates/apb-core/src/candidate/{mod,policy}.rs`, `docs/HOWTO-authoring.md`
"Supervisor policy"). A run's supervisor can write a forward patch
(`scope: next_runs`), which becomes the playbook's **candidate** version. The
run gate then picks the candidate for a start without a version when it still
holds a definition, descends from `current`, and the current version's
`supervisor.policy.trial_candidates` admits the start: `next_run` (the
default when the key is absent or unreadable) admits every start, `{ share: p }`
a random share, `off` none. The start answer then carries
`candidate_trial: "<version>"`, the candidate's version string (apb
1.0.0-beta.6 `crates/apb-server/src/routes/playbooks.rs`); a trial is
identified by the field's presence and its string value, never by a
boolean (or the answer carries `candidate_skipped` with the reason when the
candidate did not inherit trust and `current` ran).

So with the default policy a hire can run a version nobody approved for its
listing. Every playbook Nearbee materialises for a listing must declare
`supervisor.policy.trial_candidates: off` in its `current` version (apb has no
instance-wide setting for it, so `scripts/apb-instance.sh` cannot set it in
the instance config); a listing whose `current` lacks it is refused at
publish time (listing stage). The smoke playbooks are not listings and keep
the default.

### Start answers (recorded)

| Answer | When | Nearbee's class |
| --- | --- | --- |
| 200 `{run_id, ...}` | started, or queued behind a busy workdir (`workdir_queue_wait_seconds`, default 900: the run reads `running` until it gets the tree) | known start |
| 409 `{policy: "billing_requires_confirmation", billing, launch_nonce, ...}` | the run's profiles bind billed or unknown-basis agents (OpenCode on `nearai/` is unknown basis) under `billing: ask`, the default | refused; never confirmed by the runner |
| 409 `{policy: "irreversible_requires_confirmation", consent_nonce, ...}` | the playbook declares irreversible effects | refused; never confirmed |
| 409 `{error: "budget_needs_decision", estimate, unestimated, options}` | a budget whose `on_no_estimate` the request left unset (so the default `ask`) and a gated node without an estimate | refused, with code and estimate |
| 400 `invalid_budget`, 409 `notify_unavailable`, 404 | bad amount, notify target, unknown playbook or workspace | refused |
| 401, 403 | a missing or wrong API key | refused |
| 422 plain text (`Failed to deserialize the JSON body ...`) | a body of the wrong shape | refused, text as detail |
| 429 with `Retry-After` | a busy workdir when queueing is turned off (`0`) | refused |
| 5xx, 3xx (redirects are not followed), timeout, reset, refused connection, 2xx without `run_id` | | start outcome unknown: never retried; resolved by a search on `execution_id` (an empty search proves nothing unless it is complete) |

### Run detail (`GET /api/runs/{id}`)

Recorded fields Nearbee reads: `run_id`, `playbook`, `version`, `run_status`
(not `status`; the search rows use `status`), `nodes` (a map node id to node
status, `{"write": "succeeded", ...}`), `outputs` (a map node id to its output
text), `worktree` (absolute path), `params` (string values), `redacted_params`,
`usage` (`{attempts, input_tokens, output_tokens, cache_read_tokens,
cache_write_tokens, cost_usd, cost_by_basis: {billed_usd |
subscription_equivalent_usd | unknown_basis_usd}, ...}`, `null` when no
attempt reported usage; `cost_usd` is absent when no CLI reported a cost),
`budget` (limit, spent, remaining, `paused` when the budget paused the run),
`failure_reason`, `outcome`, `estimate`.

Statuses: `created`, `running`, `paused`, `interrupted` are not terminal;
`succeeded`, `failed`, `aborted` are. Nearbee treats any other value as
unknown: neither terminal nor successful.

## Params and the brief

A playbook declares `params` (text, with labels and defaults; `secret: true`
for credentials, which Nearbee never passes from a buyer). Nearbee maps a hire
to a run as:

- `instruction`: the brief text (title line plus body) as the run's instruction.
- `params`: values the listing's `input_schema` maps onto playbook params
  (language, format, repository URL, ...), parsed from the brief when the buyer
  wrote them as fields, else defaults.
- attachments: downloaded with the agent token, verified against `sha256`, saved
  into the run's worktree under `inputs/`, with the paths passed as a param.
- `worktree`: a fresh directory per hire so runs of the same playbook do not
  block each other and buyer data never mixes.
- `budget`: `usd` from the listing's cost ceiling (`docs/pricing.md`). The budget
  is a **predictive stop**: before each agent node the engine compares the
  spend so far plus the node's estimate with the limit; an admitted node can
  still overspend. Nearbee always passes `on_no_estimate` explicitly, and it
  is `continue`. Measured 2026-11-09: a request that leaves `on_no_estimate`
  unset on a playbook without history gets 409 `budget_needs_decision`; one
  that sends `ask` explicitly is accepted (200) and the run pauses before the
  first unestimated node (`budget.paused.basis: no_estimate`), and so would
  `pause`: a hire would hang there, so neither is used. A dollar limit with
  no comparable cost history cannot be weighed at all (apb warns); Nearbee
  adds its own wall-clock limit per run. Money stays `Decimal` in Nearbee;
  `usd` becomes a JSON float only in the request body.
- `hire_id` and `execution_id` as params marked `search: true` in every market
  playbook. The runner always adds both from the hire and refuses a caller
  param with either name. `execution_id` names one execution of a hire (a
  hire may run more than once: rework, redo), so a start that timed out is
  resolved by searching `field=execution_id:<id>`; `hire_id` finds every run
  of the hire. A search reports `not_indexed` (runs started before their
  definition declared search fields, or by an older apb) and `withheld`
  (values the redactor hides); only a search with both at 0 is complete, and
  only a complete empty search proves that no run exists. Nearbee's dedicated
  workspace holds only runs of playbooks with search fields: both counts were
  0 there on 2026-11-09.
- `worktree`: an existing directory that belongs to the project: a directory
  inside the workspace, or a git worktree of its repository; anything else
  refuses the start. A plain directory inside the workspace shares the
  project's busy lock (`.apb/workdir.lock`), so such runs take turns (the
  dashboard queues a second start); separate git worktrees run side by side.
  S1 uses `<workspace>/work/<name>/` (ignored by the workspace's git); S3
  decides between plain directories and git worktrees per hire.

## Results

A run's result is read from `RunDetail` (node outputs, `outputs.files`, the
goal's criteria results). The deliverable is built from a **result manifest**
the playbook writes to `out/result.json` in the worktree: the Markdown file,
the files to upload, the evidence entries, each with size and sha256. Nearbee
validates containment (paths inside `out/`), completeness and the 256 KiB
limit before submitting. **A run without a valid manifest fails closed**: the
hire is declined with `failed`; the run report or a supervisor summary is never
submitted in its place, since they can expose internal material. The manifest
is a Nearbee rule for market playbooks, not an apb feature (apb's
`outputs.files` only warns when a declared file is missing).

## Questions and gates during a run

- Interactive `agent_task` nodes ask a question and wait. Nearbee relays it to
  the buyer thread (`POST /v1/assignments/{id}/messages`), waits for the buyer's
  reply (`message.created` or poll), and answers with `POST /api/runs/{id}/answer`
  carrying the question's journal `request` seq (apb rejects stale requests),
  so Nearbee persists `(run, node, request.seq, buyer message id)` and relays
  the human answer verbatim. A question unanswered by the SLA minus a margin
  gets the node's default answer or the run fails and the hire is declined with
  `failed`. **A2A (x402) hires cannot be asked anything**: the A2A protocol has
  no multi-turn, so listings open to callers run only self-contained playbooks
  with validated defaults or an early reject. Questions are a v1.1 feature for
  web and MCP buyers; v1 ships without them.
- `human_review` gates: per listing, either the **owner** decides (apb notify to
  the owner's Telegram or Slack, decision in the dashboard) or Nearbee
  auto-approves gates the playbook marks as safe. Market playbooks should have
  few gates; the SLA clock runs while a person thinks.

## Notify as the push channel

`notify` in apb's `config.yaml` sends run transitions (review, question,
paused, finished, host_task, supervisor) through one connector function with
an idempotency declaration. Connectors are declarative HTTP (`connector.yaml`,
no code). A small `nearbee` connector (one function `post_event` to Nearbee's
loopback endpoint) makes apb push transitions instead of Nearbee polling. v1
polls `GET /api/runs/{id}`; the connector is an optimisation.

## Decisions (`apb decide`)

`apb decide <choose|rank|filter|map|is|score> "<question>" --option ... --item ... --criteria ...`
asks the configured decision providers (systemone, openrouter, vercel,
cloudflare, fake, llm_emulation). Inside a run it is journaled and budgeted.
Nearbee uses it for bounded intake decisions (is this brief within the listing's
scope; which tier does it need) through the CLI or the MCP tool `decision_ask`.
With a NEAR AI Cloud key the provider can be `typesafe/jev-1.13` on
`cloud-api.near.ai` through the OpenAI-compatible provider path (to verify;
the catalog lists it as `owned_by: typesafe`, not `nearai`, so its tier is a
question of its own). Decision providers receive the question and options, so
they are subprocessors too (`DECISIONS.md`, "What is sent").

## Profiles and model choice

A profile binds `executor: {agent, model, fallbacks[]}`, optional `tiers`,
`environment` (what of the operator's agent setup an executor loads) and a role
prompt. Nodes reference profiles by name, so swapping models never edits the
playbook. `environment` **is not a sandbox**: it does not isolate the
filesystem, the network or local credentials, and an agent or script can call
any HTTP endpoint. The CLI accepts run-level `--overrides <yaml>`, but the REST
start has no such field, so listing variants are **materialised as separate
playbook ids** with fixed profile bindings (`docs/pricing.md` § SKUs), and
Nearbee checks the executed run's playbook id and version against the listing:
the start body has no `version`, so serve starts a new run only when `GET
/api/playbooks` names the listing's recorded version as `current`, and stops
and declines `failed` any run whose detail reports another `version` or none
(the published version and digest are recorded per hire in the run intent).
Every check after a hire's run intent, also after a restart, uses the version
and digest that intent recorded, so a republish never moves a hire that
already started; serve reads the listing records again each polling round, so
the next new hire takes a republished version without a restart.

For NEAR AI Cloud models the executor path today is a CLI agent with a custom
OpenAI-compatible provider: OpenCode (`opencode.json` provider with
`baseURL: https://cloud-api.near.ai/v1`) or Codex (`[model_providers.nearai]` in
`config.toml`). apb passes the provider's environment to the executor
(`PROFILES.md`, environment table). DSH will support NEAR AI Cloud natively in a
coming apb release (owner's statement, 2026-11-09). Nearbee ships profile
templates `nearbee-own` (owner's subscription) and `nearbee-near` (OpenCode on
NEAR AI Cloud TEE models); `nearbee-near-vision` is an unsupported placeholder
(the TEE vision model has no tool calls). See `playbooks/README.md`.

### OpenCode on NEAR AI Cloud (verified 2026-11-09, OpenCode 2.2.5)

The workspace's `opencode.json` (source `playbooks/workspace/opencode.json`)
declares the provider `nearai`: `npm: @ai-sdk/openai-compatible`,
`options.baseURL: https://cloud-api.near.ai/v1`, `options.apiKey:
"{env:NEARAI_API_KEY}"`, models `Qwen/Qwen3.8-27B` and
`deepseek-ai/DeepSeek-V4.1-Flash-1017`. `opencode models`, run in the
workspace with the key in the environment, lists `nearai/Qwen/Qwen3.8-27B`
and `nearai/deepseek-ai/DeepSeek-V4.1-Flash-1017` (with the provider's whole
catalog; outside the workspace it lists no `nearai/` model). apb's own model
probe runs `opencode models` outside every workspace on purpose, so `apb
validate` reports `model_not_available` for these models: a warning, not a
refusal. An executor runs in the run's worktree inside the workspace and
finds the project's `opencode.json`.

### Billed work

OpenCode on a custom provider has an unknown billing basis for apb, so under
apb's default `billing: {billed: ask, unknown_basis: ask}` every start of a
`nearbee-near` playbook is refused with 409 `billing_requires_confirmation`
(recorded: `refusal_launch.json`). The REST answer to that is a per-run
`confirm_launch` nonce, which the runner never sends (a launch gate is the
person's decision). Nearbee's own instance therefore needs `billing: allow`
in its `policy.yaml` (`scripts/apb-instance.sh allow-billed`); spend per run
is then capped by the budget Nearbee passes. This is set only with the
owner's go-ahead for Cloud spend (O-3, O-6); until then no Cloud run starts.
Not verified yet: whether a dashboard started with a redirected
`APB_CONFIG_DIR` is also held to the default config dir's billing settings
(GUARDRAILS.md says so for run steps and CLIs without a terminal).

### Playbooks with scripts

A `script` node's file must live under the version's `scripts/`. Playbooks
are installed as bundles (`apb_bundle: 2`, YAML plus base64 scripts) through
`apb import`; a version imported with scripts runs only after `apb trust
approve` names its trust digest. A dashboard or CLI start does not gate on
playbook or profile-bundle trust otherwise.

### Measured runs

| Date | Playbook | Profile, model | Run | Status | Usage | Cost | Manifest |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-11-09 | `smoke-own` 1.0.1 | `nearbee-own`, claude `claude-haiku-5-5` (subscription) | `smoke-own-1794253399097` (fixture capture, REST) | succeeded, 8.3 s | in 6, out 557, cache read 50,384 | USD 0.00250714 subscription equivalent, not billed | `deliverable.md` 167 bytes, sha256 `da8cb8ec7897093f39ea4041fb06057b8f75d19e36362c7ee9e2d4167a51c871` |
| 2026-11-09 | `smoke-own` 1.0.1 | same | `smoke-own-1794254478357` (`ApbRunner`, `live_smoke_own`) | succeeded | in 6, out 891 | USD 0.00274908 subscription equivalent | `deliverable.md` 216 bytes, sha256 `e7246e35e048145eda622c2d15d53e0c41865e0d960920ecaad3e21ba417f096`, validated by `nearbee_core::manifest` |
| | `smoke-qwen`, `smoke-deepseek` | `nearbee-near`, `nearbee-near-deepseek` | not run | blocked: NEAR AI Cloud balance empty (O-3) | | | |

apb 1.0.0-beta.6, OpenCode 2.2.5.

## Things apb may need (feed back to apb, keep Nearbee working without them)

1. A flag on a run that marks it as **external buyer data**: skip corrections
   ledger, memory, judge-example capture and discover for it (`docs/legal.md`). Requested
   from the apb project on 2026-11-09 in its `near-ai-cloud-models` change (D-023).
2. A documented **result contract**: which node output is the result, with files.
3. A **machine estimate** of an instruction before the run (today `estimate` is
   per playbook version from history; intake needs "this brief vs this listing").
4. Executor support for NEAR AI Cloud without going through OpenCode or Codex
   custom providers (DSH native). Requested from the apb project on 2026-11-09:
   `docs/apb-request-near-models.md`.
5. A notify connector shipped with apb for a generic HTTP sink.
6. An **instance-wide switch for candidate trials** (or a start field that
   pins `current`): in 1.0.0-beta.6 `trial_candidates` is only a per-playbook
   policy, so Nearbee has to declare `off` in every materialised playbook and
   check it before publishing.
