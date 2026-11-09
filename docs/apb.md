# Agentic Playbooks (`apb`): the surface Nearbee drives

Source: https://github.com/itechmeat/agentic-playbooks (public, Apache-2.0).
Version at the time of writing: `apb 1.0.0-beta.6`. Its `docs/` folder is the
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
- Auth: `Authorization: Bearer apb_...` (keys issued as the service user);
  `X-Requested-With: apb-dashboard` is also accepted for browser calls.
- Nearbee runs on the same machine as apb and uses the loopback address with a
  key when keys are configured.

## REST endpoints Nearbee uses

| Purpose | Endpoint | Notes |
| --- | --- | --- |
| List playbooks, read one | `GET /api/playbooks`, `GET /api/playbooks/{id}` | id, versions, params (`name`, `type`, `label`, `default`, `secret`), goal, nodes |
| Estimate a run | `GET /api/playbooks/{id}/estimate?version=` | per-node and total tokens, cost and duration from comparable runs, with a text summary; the base for the listing price and SLA |
| Fit check | `GET /api/playbooks/{id}/fit` | to inspect; may serve intake |
| Start a run | `POST /api/playbooks/{id}/run` | body `{instruction?, params{k:v}, continued_from?, worktree?, confirm_irreversible?, budget{tokens?, usd?, on_no_estimate?}}`; **no `version` and no `overrides` field**: the current (gate-selected) version runs; the response carries the server-generated run id and there is no caller request id. 409 `irreversible_requires_confirmation` or `budget_needs_decision` when a gate applies |
| Follow a run | `GET /api/runs/{id}`, `GET /api/runs` | `RunDetail`: status, nodes with outcomes and outputs, goal criteria results, token usage |
| Live events | `GET /api/ws` | websocket stream the dashboard uses; polling `GET /api/runs/{id}` is enough for v1 |
| Answer a question | `POST /api/runs/{id}/answer` | `{node?, answer, request?}`; posted as `answered_by: human` |
| Decide a review gate | `POST /api/runs/{id}/review` | `{node, decision, note?, nonce?, request?}` |
| Pause, resume | `POST /api/runs/{id}/pause`, `/resume` | |
| Report | `GET /api/runs/{id}/report` | the run report (also as a page); input for the deliverable |
| Notify | `POST /api/runs/{id}/notify` | send the run's notification once |
| Profiles | `GET /api/profiles`, `GET /api/profiles/{name}` | executor agent, model, fallbacks, tiers, environment |
| Models | `GET /api/models` | known models per agent (`model_check`) |
| Stats | `GET /api/stats`, `apb stats --json` | per playbook version and node: outcome rate, retries, **median** durations and tokens, cost; estimates give medians and p25 to p75 ranges, never a p90, and cost is reported only where the executor reported usage |
| Connectors | `GET /api/connectors`, `/api/connectors/{name}` | which connectors a playbook can reach: the subprocessor list |

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
  still overspend, and with no history `on_no_estimate` defaults to `ask`, which
  makes the start return 409 `budget_needs_decision`. Nearbee passes
  `on_no_estimate: continue` for listings that have an explicit price and
  `pause` once estimates exist, and adds its own wall-clock limit per run.
- `hire_id` as a param marked `search: true` in every market playbook, so a
  run started for a hire can be found again with `apb runs --field hire_id=...`
  after a timeout on the start call (the start has no caller request id).

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
Nearbee checks the executed run's playbook id and version against the listing.

For NEAR AI Cloud models the executor path today is a CLI agent with a custom
OpenAI-compatible provider: OpenCode (`opencode.json` provider with
`baseURL: https://cloud-api.near.ai/v1`) or Codex (`[model_providers.nearai]` in
`config.toml`). apb passes the provider's environment to the executor
(`PROFILES.md`, environment table). DSH will support NEAR AI Cloud natively in a
coming apb release (owner's statement, 2026-11-09). Nearbee ships profile
templates for `own` (owner's claude or codex subscription), `near` (OpenCode on
NEAR AI Cloud TEE models) and `near-vision` (Qwen3-VL for OCR-like steps).

## Things apb may need (feed back to apb, keep Nearbee working without them)

1. A flag on a run that marks it as **external buyer data**: skip corrections
   ledger, memory, judge-example capture and discover for it (`docs/legal.md`).
2. A documented **result contract**: which node output is the result, with files.
3. A **machine estimate** of an instruction before the run (today `estimate` is
   per playbook version from history; intake needs "this brief vs this listing").
4. Executor support for NEAR AI Cloud without going through OpenCode or Codex
   custom providers (DSH native). Requested from the apb project on 2026-11-09:
   `docs/apb-request-near-models.md`.
5. A notify connector shipped with apb for a generic HTTP sink.
