# privacy-tiers Specification

## Purpose
Defines the privacy tiers a listing declares, the execution facts Nearbee checks a tier against, and when the check runs, so a listing never promises more than its playbook's bindings allow.

## Requirements

### Requirement: Tiers
A listing SHALL declare exactly one tier, `plain` or `attested`. `plain` promises only the data statement. `attested` promises that every model step is configured to run on NEAR AI Cloud Confidential TEE models and that the deliverable carries endpoint attestation evidence. No other tier value SHALL be accepted.

#### Scenario: Unknown tier
- **WHEN** a listing file says `tier: blind`
- **THEN** loading fails naming the field and the allowed values `plain` and `attested`

### Requirement: Execution facts
The check SHALL use facts read from Nearbee's apb instance for the installed playbook: its nodes (kind, interactive flag, profile reference with its scope, connectors, playbook references), every profile a node can use with its executor, ordered fallbacks and tiers, environment, skills and process environment policy, the model routes, and the decisions and compaction settings. A fact apb cannot provide SHALL be unknown, never guessed.

#### Scenario: Fallbacks are read
- **WHEN** a profile's executor is a TEE model and its fallback is `claude-haiku-5-5`
- **THEN** the facts list both models and the attested check sees the fallback

#### Scenario: Scoped profile reference
- **WHEN** a node names `{name: nearbee-near, scope: global}` and a project profile of the same name exists
- **THEN** the facts hold the global profile, not the project one

#### Scenario: Unreadable profile
- **WHEN** apb answers 404 for one profile the playbook names, or its YAML does not parse
- **THEN** that profile's facts are unknown

#### Scenario: Malformed fallback
- **WHEN** a profile's fallback or tier entry lacks its agent or model, or the list is not a list
- **THEN** the whole profile is unknown; the entry is never dropped in silence

#### Scenario: Failed read
- **WHEN** a read of the apb instance fails on transport or a server error (a profile, the health route, the unit environment)
- **THEN** the inspection fails as a whole instead of reporting an unknown fact

### Requirement: Structural checks for every tier
Every listing SHALL be refused when its playbook has a `human_review` node, a node marked interactive, a node kind the check does not know, or a binding to another playbook; and when any fact the statement needs is unknown, the decisions and compaction settings included.

#### Scenario: Review gate
- **WHEN** the playbook contains a `human_review` node
- **THEN** the listing is refused naming the node, whatever its tier

#### Scenario: Interactive agent task
- **WHEN** an `agent_task` node sets `interactive: true`
- **THEN** the listing is refused naming the node

#### Scenario: Plain listing with decisions unknown
- **WHEN** a `plain` listing's decisions or compaction setting cannot be read
- **THEN** the listing is refused naming the setting

### Requirement: Attested model route
An attested listing SHALL be refused when its playbook has any `agent_task` or `prompt` node, whatever its profile, and when any usable profile's executor, fallback or tier runs an agent (OpenCode runs in a shared background service with its starter's configuration; no agent route is verified). Its model calls SHALL be only the direct and signed calls of Nearbee's own commands from script nodes.

#### Scenario: OpenCode on an attested listing
- **WHEN** an attested listing's profile runs `opencode` on a catalog `nearai/` model
- **THEN** the listing is refused with a breach naming the shared OpenCode service and saying attested listings use Nearbee's direct calls only

#### Scenario: Another agent on a TEE model
- **WHEN** an attested listing's profile falls back to `claude` on a catalog `nearai/` model
- **THEN** the listing is refused naming the profile

#### Scenario: Agent node without a profile
- **WHEN** an attested listing's playbook has an `agent_task` node with no profile and no default profile
- **THEN** the listing is refused naming the node

#### Scenario: Script nodes only
- **WHEN** an attested listing's playbook has only script nodes and every direct model is a catalog model owned by `nearai`
- **THEN** the route check passes

#### Scenario: Incognito fallback
- **WHEN** an attested listing's profile falls back to a model the catalog owns as `anthropic`
- **THEN** the attested listing is refused (its profile runs an agent)

#### Scenario: Wrong route
- **WHEN** the `nearai` provider's base URL is any other address
- **THEN** the attested listing is refused (its profile runs an agent)

#### Scenario: OpenCode override
- **WHEN** the apb instance's environment sets an `OPENCODE_` configuration variable
- **THEN** the attested listing is refused (its profile runs an agent)

#### Scenario: Key reference
- **WHEN** the `nearai` provider's `apiKey` is anything but `{env:NEARAI_API_KEY}`
- **THEN** the attested listing is refused (its profile runs an agent)

#### Scenario: Unknown model
- **WHEN** a profile names `nearai/some-new-model` that the snapshot does not list
- **THEN** the attested listing is refused (its profile runs an agent)

### Requirement: Model entries
An attested listing SHALL be refused when the `nearai` provider declares a model entry with any key beyond display and metadata (such as `id` or `options`), since such an entry aliases or overrides what a catalog id calls.

#### Scenario: Model alias
- **WHEN** the `nearai` provider maps a catalog id to another model with an `id` key
- **THEN** the attested listing is refused naming the entry

### Requirement: OpenCode configuration
The model routes SHALL be unknown when OpenCode would read configuration other than the workspace's `opencode.json`: the instance's `XDG_CONFIG_HOME` is not Nearbee's own directory (`apb.xdg_config_home`) or that directory holds an `opencode` entry, the workspace has a `.opencode/` directory, `/etc/opencode` exists, or a directory above the workspace holds `opencode.json`, `opencode.jsonc` or a `.opencode/` with more than OpenCode's install files.

#### Scenario: Owner's global OpenCode config
- **WHEN** the apb unit runs with the owner's `XDG_CONFIG_HOME`, or Nearbee's directory holds `opencode/opencode.json`
- **THEN** the routes are unknown and the attested listing is refused

#### Scenario: Workspace plugin directory
- **WHEN** the workspace holds `.opencode/plugins/`
- **THEN** the routes are unknown and the attested listing is refused

### Requirement: Attested side channels
An attested listing SHALL be refused when any usable profile has skills, an environment other than `minimal`, or a process environment policy other than an allowlist naming at most `NEARAI_API_KEY`; when the playbook binds a connector; when decisions are enabled or unknown; or when context compaction is enabled or unknown (unknown refuses every tier, see Structural checks).

#### Scenario: Decisions on
- **WHEN** the apb project's decisions are enabled
- **THEN** the attested listing is refused saying the scope check would send the brief to a decision provider

#### Scenario: Compaction unknown
- **WHEN** the compaction setting cannot be read
- **THEN** the attested listing is refused

### Requirement: What the check does not prove
The check SHALL cover declared bindings only. Every refusal and every statement SHALL treat egress the bindings do not declare (a script or agent calling any address with any local credential) and an agent step reading the owner's files as not prevented: apb 1.0.0-beta.6 runs no `opencode` step in a sandbox.

#### Scenario: Statement wording
- **WHEN** an attested statement is generated
- **THEN** it says the run happens on the owner's machine, which can read the data, that the agent steps run without a sandbox, and that reading the owner's files and undeclared network access are not prevented

### Requirement: When the check runs
The check SHALL run in `nearbee list` on the materialised source before installing and again on the installed version read back from apb, and in `serve` once per round for every listing, before intake or any start can send buyer data. A refusal at publish SHALL leave the previous listing record unchanged and exit non-zero. When the facts cannot be read in a `serve` round, the listing SHALL be held for that round (spec `hire-serving`, Held listing), never served unchecked.

#### Scenario: Refused after install
- **WHEN** the installed read back fails the check that the source passed
- **THEN** the command exits non-zero, the listing record is not updated, and the output names the failed check

### Requirement: Direct models
A listing's `direct_models` SHALL be the only models its Nearbee commands may call from script nodes. An attested listing SHALL be refused unless every direct model is in the catalog snapshot with owner `nearai`; a plain listing SHALL be refused when a direct model is not in the snapshot.

#### Scenario: Incognito direct model
- **WHEN** an attested listing lists `anthropic/claude-sonnet-5` as a direct model
- **THEN** the listing is refused naming the model

#### Scenario: Off-catalog direct model on a plain listing
- **WHEN** a plain listing lists a model absent from the catalog snapshot
- **THEN** the listing is refused naming the model

#### Scenario: Command outside the list
- **WHEN** a digest command is asked to call a model the hire's frozen direct models do not list
- **THEN** the command fails before any request

### Requirement: Direct call route
The model routes SHALL be unknown when the apb instance environment sets `NEARAI_CLOUD_URL`, any `NEARBEE_CLOUD_` variable, `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` (any case); `nearbee list` then refuses an attested listing and the drift pass reports it drifted. Nearbee's commands SHALL call only `https://cloud-api.near.ai/v1`, without a proxy.

#### Scenario: Redirecting variable
- **WHEN** the instance environment sets `NEARAI_CLOUD_URL`
- **THEN** `nearbee list` refuses the attested listing naming unknown routes

### Requirement: Compaction by apb version
The compaction setting SHALL be known off only for the apb versions whose sources were read for it, `1.0.0-beta.6` and `1.0.0-beta.7` (no start surface sets `context_max_bytes`), compared as parsed semantic versions ignoring build metadata, and SHALL be unknown for every other version and for a string that is not a version, so a newer apb is refused for every tier until it is read.

#### Scenario: beta.7
- **WHEN** the instance reports `1.0.0-beta.7`
- **THEN** compaction is known off

#### Scenario: Build metadata
- **WHEN** the instance reports `1.0.0-beta.7+abc`
- **THEN** compaction is known off

#### Scenario: Unread release
- **WHEN** the instance reports `1.0.0-beta.8`
- **THEN** compaction is unknown and every tier is refused

### Requirement: Audit review model
The fast repository audit's `nearbee audit review` SHALL call only a model in the hire's frozen direct models (the `direct_models` run param) and SHALL fail before any request otherwise; the signed models are enforced by serve's signature recheck at delivery.

#### Scenario: Model outside the list
- **WHEN** the `direct_models` run param does not list DeepSeek V4.1 Flash
- **THEN** `nearbee audit review` fails before any request

### Requirement: Sandbox fact
The runner's inspection SHALL report a sandbox fact only when `systemctl --user show nearbee-apb -p ExecStart` shows a `docker run` of an image named `nearbee-sandbox:<tag>` whose arguments include `--read-only`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--dns 127.0.0.1` and `--network <name>`; its hosts SHALL be the sorted, deduplicated names of every `--add-host <name>:<ip>`. A failed read or any other command line SHALL yield no sandbox fact.

#### Scenario: Sandboxed unit
- **WHEN** the unit's ExecStart is the sandbox's `docker run` with three `--add-host` entries
- **THEN** the fact lists those three host names sorted

#### Scenario: Missing limit
- **WHEN** the `docker run` lacks `--cap-drop ALL`
- **THEN** there is no sandbox fact

### Requirement: Facts without a sandbox
Facts without a sandbox fact SHALL serialize exactly as before this change.

#### Scenario: Unchanged fingerprint
- **WHEN** the instance runs without a sandbox
- **THEN** the facts fingerprint equals the fingerprint the same facts had before this change

### Requirement: Script models are plain only
An attested listing with a non-empty `script_models` SHALL be refused, because a script's own CLI call cannot be checked; a plain listing SHALL accept it. Facts with an empty `script_models` SHALL serialize exactly as before this change.

#### Scenario: Attested listing with script models
- **WHEN** an attested listing declares `script_models: [claude/claude-opus-5-5]`
- **THEN** the listing is refused with a reason that names `script_models` and tier plain

#### Scenario: Unchanged fingerprint
- **WHEN** a listing without `script_models` is fingerprinted
- **THEN** the fingerprint equals the one the same facts had before this change
