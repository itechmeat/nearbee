# Agentic Playbooks (`apb`): the surface Nearbee drives

Source: https://github.com/itechmeat/agentic-playbooks (public, Apache-2.0).
Version at the time of writing: `apb 1.0.0-beta.7` (installed on Nearbee's instance since 2026-11-11, S6; Nearbee needs at least this release). Facts recorded in S1 to S5 were read at `1.0.0-beta.6` and say so; the beta.7 facts are in "apb 1.0.0-beta.7" below. Its `docs/` folder is the
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
| `install [<workspace>]` | copies `playbooks/profiles/*.yaml` as project profiles (`.apb/profiles/<name>/profile.yaml` plus a one-line `SOUL.md`) and `playbooks/workspace/opencode.json` to the workspace root through `scripts/apb-workspace-write.py`, which follows no symlink, and imports each `playbooks/smoke/*.yaml` with its `scripts/` as a bundle on stdin (`apb import /dev/stdin --accept <bundle digest>`, the digest the sha256 of the bundle, computed by the script), then approves the version, also an unchanged one, by its trust digest (`apb trust approve <id> --version <v> --digest <d>`): a version imported with scripts does not run until approved. Before and after the approval, `apb import <bundle> --preview` must say the current version is `<v>` and already holds the bundle just built (apb compares definition and scripts); otherwise nothing is approved, or the approval just made is revoked. A version that exists with other content (apb: "already exists") stops the install with "bump the version". Prints `playbook <id> <v> approved` per playbook and exits non-zero naming the playbook when apb prints no digest or refuses it |
| `allow-billed` | writes `billing: {billed: allow, unknown_basis: allow}` to the instance's `policy.yaml` in place, never through a symlink, and only over a file this script wrote (its first line `# nearbee: ...`); see "Billed work" below. Only with the owner's go-ahead for Cloud spend |
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

### Sandboxed instance (change apb-sandbox, 2026-11-12)

With `NEARBEE_APB_SANDBOX=1` the same script runs the instance inside a
container with an outbound allowlist; the default mode above is unchanged.
It needs Docker. Setup:

```sh
sandbox/build.sh                       # image nearbee-sandbox:local
export NEARBEE_APB_SANDBOX=1
scripts/apb-instance.sh register ~/.local/share/nearbee/apb-workspace --git
scripts/apb-instance.sh install
scripts/apb-instance.sh start          # issues a server key on first start
export APB_BIN="$PWD/scripts/apb-sandboxed"   # absolute path, for nearbee
```

1. Image (`sandbox/Dockerfile`, `sandbox/build.sh`): Ubuntu 26.04 (the host
   release, so copied binaries find the same glibc) with git, gh, ripgrep,
   jq, curl, python3, poppler-utils, ImageMagick, Node with OpenSpec 2.0.0
   and OpenCodeReview 1.13.8-fork.1, Code Ranker 5.0.4, and the host's
   `apb`, `nearbee` and self-contained `claude`, `codex` and `opencode`
   binaries (`NEARBEE_SANDBOX_CLIS` changes the list; a CLI that is a
   script is skipped). The image is labelled with the sha256 of the copied
   `apb` and `nearbee`; `start` refuses when the host's binary (`APB_BIN`,
   `NEARBEE_BIN` or PATH) differs and names the rebuild command.
2. Networks: `nearbee-sbx-int` (internal, `NEARBEE_SANDBOX_SUBNET`, default
   `172.30.77.0/24`, gateway mode `isolated`: the host has no address on
   its bridge, so the container cannot reach host services at `.1` either;
   measured 2026-11-12, every host port tried at `.1` is closed) and
   `nearbee-sbx-out`. A start makes `nearbee-sbx-int` again when it exists
   with another subnet or another gateway mode. The gateway `nearbee-apb-gw`
   (`nginx:1.29-alpine`, `--restart unless-stopped`, as the image's `nginx`
   user with no capability at all, `no-new-privileges`, read-only root,
   tmpfs `/tmp` and `/var/cache/nginx`) sits on both at `.2`;
   the dashboard container `nearbee-apb-sbx` at `.3` on the internal one
   only. The gateway reads the TLS server name (`ssl_preread`) and passes
   the connection through unchanged to that host when it is in
   `sandbox/egress.allow` or `NEARBEE_SANDBOX_ALLOW`, else to a closed port;
   it also publishes the dashboard on `127.0.0.1:$NEARBEE_APB_PORT` only.
   A gateway is reused only when its configuration is unchanged and it is
   attached at `.2`; one whose attach failed is removed at once.
   The container's hosts file maps each allowed host to the gateway.
   Docker's embedded resolver answers inside the internal network but
   resolves no outside name (measured: `example.com` does not resolve, a
   raw IP has no route).
3. Container: the owner's uid and gid, read-only root, `/tmp` a 1 GB tmpfs,
   all capabilities dropped, `no-new-privileges`, 1024 processes, 4 GB,
   4 CPUs. Mounts, each at its resolved host path: the workspace
   (`NEARBEE_APB_WORKSPACE`, read-write), the config directory (read-write,
   with `policy.yaml`, `server-auth.yaml` and `run-origin.key` bound
   read-only over it when they exist; a start creates `policy.yaml` with
   no settings when it is missing, so it is always bound, and refuses any
   of the three that is a symlink, not a regular file of the owner, or
   does not resolve to its own path), the unit's `XDG_CONFIG_HOME`
   (read-only), the sandbox home `~/.local/share/nearbee/sandbox-home`
   (`NEARBEE_SANDBOX_HOME`, read-write, with an empty read-only directory
   over its `.opencode`; `start` refuses entries there), and each path of
   `NEARBEE_SANDBOX_MOUNTS_RO` (read-only; space separated, never
   globbed, and a path with a comma, a quote or a newline is refused,
   since it would add options to `docker --mount`). Nothing else of the
   host home.
4. Unit: still `nearbee-apb` (the tier check reads it). Its `Environment=`
   entries are the ones the container receives with the same values
   (`APB_CONFIG_DIR`, `XDG_CONFIG_HOME`, `HOME` the sandbox home,
   `NEARBEE_BIN=/usr/local/bin/nearbee`, the container `PATH`, the nearbee
   git identity) plus `DOCKER_CONFIG` (a Nearbee-owned empty directory, for
   the docker client only, never passed in); the provider keys come from
   the same `EnvironmentFile` as in the default mode. `ExecStopPost`
   removes the container, so a killed client leaves none (checked by
   `kill -9` on the client).
5. Server key: apb refuses a non-loopback bind without one. `start` issues
   a key only when the instance has none and writes it as `APB_API_KEY` to
   `NEARBEE_ENV_FILE` (keep that the `.env` next to `nearbee.yaml`), never
   printing it and never passing it into the container; with a key issued
   but no `APB_API_KEY` in that file it stops and says so.
   `NEARBEE_APB_ENV_VARS` may not name `APB_API_KEY`, a variable the
   sandbox sets itself (systemd's `EnvironmentFile=` would override the
   unit's value, which the tier check reads, while the container got the
   `.env` value) or a `DOCKER_` variable. The health check
   reads the header from a 0600 file (`curl -H @file`). `/api/health`
   answers without a key; every other route needs it. Changing keys
   (`apb server key issue|revoke`) needs the instance stopped, because the
   key file is bound read-only into the running container.
6. CLI calls: the script's `apb`, `register` and `install`, and
   `scripts/apb-sandboxed` (Nearbee's `APB_BIN`, named by its absolute
   path) run apb with `docker exec` in the running container, or in a
   one-off container on no network while it is down, from the caller's
   directory when it is mounted (else the workspace), with
   `timeout --kill-after=5 $NEARBEE_SANDBOX_CLI_TIMEOUT` (default 25 s)
   inside the container. Host code never reads or writes a file in the
   workspace, the config directory or the sandbox home that a step could
   have swapped or turned into a symlink: steps write those under the
   owner's uid, and a process a step left behind lives until `stop`. The
   bundles of `install` and of `nearbee list` reach apb on stdin
   (`apb import /dev/stdin`), with the accept digest computed from the
   bundle itself, and the read-back comes from `apb export` on stdout;
   `install` writes the profiles, `opencode.json` and the
   `decisions: { enabled: false }` edit through
   `scripts/apb-workspace-write.py` inside the container (no symlink is
   followed, every file goes to a fresh random name and is renamed into
   place); `register` makes the directory and runs git inside the
   container. A start removes a leftover dashboard container before it
   checks the shared directories. `start`
   refuses when `nearbee.yaml` (or `nearbee.local.yaml`) next to the `.env`
   names another `apb.workspace_path` than the mounted workspace.
7. `stop` interrupts every run (the drivers live in the container); the
   next `start` runs `apb resume --all` in the workspace once the dashboard
   answers. A failed start removes the key file it wrote. `down` also
   removes the gateway and both networks. `status`
   shows the unit, both containers, whether the image matches the host
   binaries and the health answer.
8. Which config files apb writes, measured with a full run of apb's keyless
   demo in the container (2026-11-12): only `projects.json` (replaced by
   rename on every project touch) and the run's own state; `trust.json`
   changes on an import or approval, also by rename. A file bound read-only
   pins its inode, so a rename-written file cannot be: `trust.json` and
   `projects.json` stay writable (design 2.5 listed them read-only; the
   approval through `scripts/apb-sandboxed` and every run would fail).
9. `NEARBEE_APB_UNIT` (default `nearbee-apb`) renames the unit, its
   containers (`<unit>-sbx`, `<unit>-gw`) and networks; it exists only to
   test a second instance next to a running one, since the tier check reads
   `nearbee-apb`.
10. The data statement reads the sandbox from the unit itself (change
    statement-sandbox, `docs/privacy.md`, "The execution facts"): the
    inspection parses `systemctl --user show nearbee-apb -p ExecStart` and
    names the container and the `--add-host` names in "Where the run
    happens". Moving an instance into or out of the sandbox changes the
    facts fingerprint, so run `nearbee list` again and paste the new
    statement.

Limits, stated as they are:

1. One container per instance, not per hire: hires share it and are kept
   apart by worktrees only. A step can leave files in the shared sandbox
   home and in the writable parts of the config directory (including
   `trust.json` and `projects.json`) that later hires see.
2. A sandboxed `stop` interrupts every run; `start` resumes them.
3. Containers share the host kernel.
4. The allowlist is by TLS server name. An allowed host that accepts
   uploads (GitHub with a token) remains a channel out, and a host behind a
   shared CDN allows domain fronting.
5. OpenCode fetches from `registry.npmjs.org` and `models.dev` at run time
   unless its cache is warm in the sandbox home: add them to
   `NEARBEE_SANDBOX_ALLOW`, or warm the cache once with them allowed.
6. Agent CLI logins go in as API keys or tokens listed in
   `NEARBEE_APB_ENV_VARS` (for example `CLAUDE_CODE_OAUTH_TOKEN`), never as
   the owner's config directories; a CLI that needs more hosts than the
   default allowlist needs them in `NEARBEE_SANDBOX_ALLOW`.

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

### CLI calls Nearbee makes (apb 1.0.0-beta.6)

| Call | Wording and exits (recorded at the tag) |
| --- | --- |
| Abort a run: `apb stop <run_id>` | `stopping <id>: abort sent to the running driver` (exit 0, a live driver gets the abort); `stopped <id>: no driver was running, the run is now aborted` (exit 0, a driverless run is finalized as aborted); `<id> had already finished, so no run was stopped` (exit 0, also covers a run that reached a terminal state while the stop was in flight); `stop failed: ...` on stderr (exit 2). Nearbee runs it from the workspace with its instance's `APB_CONFIG_DIR`, 30 s timeout, and repeats it each round until an observation reads terminal: the stop is idempotent on a finished run. `POST /api/runs/{id}/pause` is a drain pause and no longer used by serve |
| Ask a decision: `apb decide is <question> --criteria <text> --no-run` | one JSON line on stdout; exit 0 when answered, 1 when refused or failed, 2 on a malformed request. Answered: `{"kind":"is","answer":"yes"|"no","p":<probability of yes>,...,"answered":true}`. Refused: `{"answered":false,"refused":"<code>","reason":"..."}` (measured on Nearbee's instance 2026-11-10: `{"answered":false,"refused":"no_provider","reason":"no decision provider is configured"}`). Failed: `{"answered":false,"error":"<...>","reason":"..."}`. `--no-run` asks outside any run even when `APB_RUN_ID` is set; Nearbee also removes `APB_RUN_ID` and `APB_NODE_ID` from the child's environment (a provider call may cost money, and apb keeps no replay cache for decisions outside a run). Each argument is passed directly, no shell |

### Facts about starts and runs (recorded 2026-11-10)

- `continued_from: "<run_id>"` in the start body continues an earlier run of
  the same playbook (a rework or a redo); the start answer is shaped as any
  start's.
- The first `GET /api/runs/{id}` of a freshly started run can carry
  `"version": null` even though every later detail names the version: the
  provenance event is not in the answer yet (measured on Nearbee's instance,
  2026-11-10). A version check therefore allows a pending first observation
  for a short time and only judges a terminal run or one observed without a
  version for more than 60 seconds.
- `usage` carries the counters `attempts` (attempts whose output reported
  usage), `finished_attempts` (every attempt the run finished, with or
  without usage) and `cost_attempts` (how many of `attempts` reported a
  cost), so a partial sum reads as one; `cost_usd` is the mixed sum and
  `cost_by_basis` splits it. A run's cost is complete only when it is
  terminal and `cost_attempts == finished_attempts`.
- There is no route to delete a run or its artifacts at the tag (no
  `DELETE /api/runs/{id}`): apb run directories of finished hires stay on
  disk and are listed as not covered by `nearbee cleanup`.

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

### Inspection routes (recorded 2026-11-11, S4)

Recorded from Nearbee's instance (`1.0.0-beta.6`) into
`crates/nearbee-apb/tests/fixtures/apb/*_live.json` (home paths replaced
by `/home/user`):

1. `GET /api/health`: `{build_id, status, version}`; `version` is the apb
   release the compaction fact depends on.
2. `GET /api/playbooks/{id}?workspace=<ws>`: `id`, `version`, `yaml` (the
   installed text), `playbook` (parsed), `validation`, `frozen`,
   `assumed_effects`, `effective_effects`, `edge_addresses`, `layout`.
3. `GET /api/profiles/{name}?workspace=<ws>&scope=project|global`: `name`,
   `scope`, `profile_yaml`, `soul_md`, `profile_digest`, `bundle_digest`.
   A profile missing in that scope answers 404 with the plain text
   `profile `<name>` not found`. Nearbee's profiles are all project scope;
   the instance has no global profiles.

### apb 1.0.0-beta.7 (read 2026-11-11, S6)

1. Health: `GET /api/health` answers `{"build_id": "ef8c081aa53b61e8",
   "status": "ok", "version": "1.0.0-beta.7"}` (recorded:
   `health_beta7_live.json`). `nearbee-apb` parses `version` as a semantic
   version with prerelease order (`semver`; `1.0.0-beta.10` is above
   `1.0.0-beta.7`, `1.0.0` above both); a missing or unparsable version is
   a protocol error naming the field. The core sees one bool,
   `Runner::external_data_supported`: true from `1.0.0-beta.7` on.
2. External-data runs (apb `docs/EXTERNAL-DATA.md`): a start body with
   `"external_data": true` marks the run as carrying outside data, so apb
   skips its learning state for it (corrections ledger, memory, judge
   examples, discovery). The run detail then carries a top-level
   `"external_data": true`, and its `run_started` event carries the same
   field (recorded: `run_external_data_live.json`, a throwaway script-only
   playbook started once over REST on Nearbee's instance with the flag, no
   agent and no model, then deleted from the workspace). A detail without
   the field, as every beta.6 detail, is not an external-data run. apb
   flags a continued run itself: "a run started with `continued_from`
   naming a flagged run" is flagged (`docs/EXTERNAL-DATA.md` at
   `v1.0.0-beta.7`); Nearbee sends the flag explicitly on every start
   anyway, reworks and redos included. Serve records the flag on the run
   start intent and checks it on every observed run detail of a run whose
   intent recorded it; a run started before that ref existed is not
   checked, so an upgrade does not abort hires in flight
   (`docs/privacy.md`, External-data runs); the live smoke test and
   `examples/capture.rs` send no flag (they carry no buyer data).
3. Compaction, re-read at `v1.0.0-beta.7`: `context_max_bytes` is still
   set by no surface. Every `RunOptions` constructor in `apb-cli`,
   `apb-mcp`, `apb-server`, `judge_eval` and the sub-playbook child uses
   `..Default::default()`; `apb-cli/src/run.rs` sets `None`; `RunConfig`
   is written only by `prepare.rs` from those options. So compaction is
   known off for exactly `1.0.0-beta.6` and `1.0.0-beta.7`, compared as
   parsed semantic versions ignoring build metadata (`1.0.0-beta.7+abc`
   counts) (`nearbee_apb::compaction_for_version`); any other version, or
   a string that is not a version, is unknown,
   which refuses every tier until someone reads that release too.
4. The native `nearai/` route: beta.7 runs NEAR AI Cloud models through
   its own `dsh` harness (`docs/AGENT-SETUP.md`, "DeepSeek Harness").
   Nearbee does not use it: `dsh` reads `~/.agents/skills`, mounts the home
   patch layer where `apb setup` registers the MCP server, has no
   `minimal` environment and no sandbox, so an attested listing could not
   vouch for it. Attested listings make their model calls from Nearbee's
   own commands instead (D-027).
5. OpenCode as apb starts it: `run --format json -m <model> -- <prompt>`
   without `--standalone` (`crates/apb-core/src/agents/opencode/mod.rs`
   at the tag). OpenCode 2.x then hands the session to a shared background
   service (`opencode serve --service`) that runs with the environment of
   whoever started it; probed on this machine on 2026-11-11 (OpenCode
   2.2.5, no model call: a missing model id fails before any request), it
   ran with the owner's home and configuration and connected the owner's
   `agentic-playbooks` MCP server. With `--standalone` it honours
   `XDG_CONFIG_HOME` but still loads `~/.agents/skills` and
   `~/.opencode/skills`. The attested tier therefore refuses `opencode`
   (`docs/privacy.md`, Attested tier without agents).

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
templates `nearbee-own` (owner's subscription), `nearbee-own-premium` (Claude
Opus on the owner's subscription, since S4) and `nearbee-near` (OpenCode on
NEAR AI Cloud TEE models: DeepSeek V4.1 Flash first, Qwen3.8-27B as
fallback since S4); `nearbee-near-vision` is an unsupported placeholder
(the TEE vision model has no tool calls). See `playbooks/README.md`.

### What S4 relies on (read at `v1.0.0-beta.6`)

Since S6 no attested listing runs an agent (D-027), so items 3 and 4 below
matter only for the statement of a plain listing that uses OpenCode; none
ships. They are kept, and still checked, as facts.

1. Script nodes run in the run's worktree, inherit the dashboard's
   environment (no `env_clear`) and get `APB_RUN_DIR`, `APB_RUN_ID` and
   `APB_NODE_ID` (`crates/apb-engine/src/script.rs`, `run_env` and
   `run_script_with_env_removed`). `nearbee audit fetch` reads the run's
   params from `$APB_RUN_DIR/run.yaml` (`crates/nearbee-apb/src/run_params.rs`),
   and the scripts find the binary through `NEARBEE_BIN`, which
   `scripts/apb-instance.sh start` sets in the unit.
2. The project's `.apb/config.yaml` carries `decisions: { enabled: false }`
   (`scripts/apb-instance.sh install`); the tier check reads it, and the
   intake scope check is skipped while it is off (`docs/privacy.md`).
3. No `OPENCODE_` variable may reach the instance: OpenCode reads them over
   `opencode.json`, so the tier check treats them as unknown routes, and
   `scripts/apb-instance.sh start` refuses to pass or inherit one.
4. The unit runs with `XDG_CONFIG_HOME` set to Nearbee's own empty directory
   (`~/.local/share/nearbee/xdg-config`, `NEARBEE_APB_XDG_CONFIG_HOME`; the
   config key `apb.xdg_config_home` must name the same path), so OpenCode
   merges no global config of the owner: apb passes the `XDG_*_HOME`
   variables under a `process_env` allowlist (`docs/PROFILES.md` at the
   tag), and apb's own OpenCode code writes `$XDG_CONFIG_HOME/opencode/opencode.json`
   (`crates/apb-core/src/agents/opencode/mcp.rs`). The inspection reads the
   unit environment (`SYSTEMCTL_BIN` names another `systemctl`) and makes the
   routes unknown unless that directory is set and holds no `opencode`
   entry, the workspace has no `.opencode/`, `/etc/opencode` is absent and
   no directory above the workspace holds OpenCode configuration
   (`docs/privacy.md`, OpenCode configuration). OpenCode still loads skills
   and instructions from `~/.claude` and `~/.agents`; that gap is recorded
   there.
5. apb's opencode adapter declares no sandbox needs at the tag
   (`Capabilities::NONE`, no `HostSetup` needs), so `isolation: full`
   refuses an opencode node (`agent_needs_unknown`) and `best_effort` runs it
   without a sandbox in a scratch directory without the project tree
   (`docs/ISOLATION.md`). The repo-audit `audit` node therefore declares
   only `writes`, `max_changed_files` and `protect`, which apb enforces
   after every attempt (`docs/privacy.md`, Repository audit).
6. A failed read of the instance during the inspection (a transport error,
   a 5xx, a failed `systemctl`) fails the inspection; `serve` holds the
   listing for that round instead of declining its hires.

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
refusal. `apb validate [NAME]` at the tag checks installed playbooks of the
project in the working directory by id, not a source file: run it in the
workspace with Nearbee's `APB_CONFIG_DIR` after `nearbee list` (measured
2026-11-10: `apb validate repo-audit-fast` answers `repo-audit-fast: OK`). An executor runs in the run's worktree inside the workspace and
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
   from the apb project on 2026-11-09 in its `near-ai-cloud-models` change (D-023);
   shipped in `1.0.0-beta.7` as `external_data` and used on every market run
   since S6 (above).
2. A documented **result contract**: which node output is the result, with files.
3. A **machine estimate** of an instruction before the run (today `estimate` is
   per playbook version from history; intake needs "this brief vs this listing").
4. Executor support for NEAR AI Cloud without going through OpenCode or Codex
   custom providers (DSH native). Requested from the apb project on 2026-11-09:
   `docs/apb-request-near-models.md`. Shipped in `1.0.0-beta.7` as the
   native `nearai/` route on `dsh`; not used by Nearbee (above, item 4 of
   "apb 1.0.0-beta.7"). Still missing for an attested agent route: an
   OpenCode start with `--standalone` and a separate home, or a `dsh`
   environment that reads nothing from the owner's home.
5. A notify connector shipped with apb for a generic HTTP sink.
6. An **instance-wide switch for candidate trials** (or a start field that
   pins `current`): in 1.0.0-beta.6 `trial_candidates` is only a per-playbook
   policy, so Nearbee has to declare `off` in every materialised playbook and
   check it before publishing.
