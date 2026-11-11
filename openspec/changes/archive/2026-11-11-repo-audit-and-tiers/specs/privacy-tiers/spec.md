# Spec Delta

## Purpose
Defines the privacy tiers a listing declares, the execution facts Nearbee checks a tier against, and when the check runs, so a listing never promises more than its playbook's bindings allow.

## ADDED Requirements

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
An attested listing SHALL be refused unless every executor, fallback and tier of every usable profile runs the agent `opencode` on a model `nearai/<id>`, the workspace's `nearai` provider points exactly at `https://cloud-api.near.ai/v1` with its key read as `{env:NEARAI_API_KEY}` and no model alias (see Model entries), no OpenCode configuration other than the workspace's `opencode.json` applies to the apb instance, and `<id>` is in Nearbee's dated catalog snapshot with owner `nearai`.

#### Scenario: Another agent on a TEE model
- **WHEN** an attested listing's profile falls back to `claude` on a catalog `nearai/` model
- **THEN** the listing is refused naming the profile and saying only `opencode` runs

#### Scenario: Incognito fallback
- **WHEN** an attested listing's profile falls back to a model the catalog owns as `anthropic`
- **THEN** the listing is refused naming the profile and the model

#### Scenario: Wrong route
- **WHEN** the `nearai` provider's base URL is any other address
- **THEN** the attested listing is refused naming the route

#### Scenario: OpenCode override
- **WHEN** the apb instance's environment sets an `OPENCODE_` configuration variable
- **THEN** the routes are unknown and the attested listing is refused

#### Scenario: Key reference
- **WHEN** the `nearai` provider's `apiKey` is anything but `{env:NEARAI_API_KEY}`
- **THEN** the attested listing is refused

#### Scenario: Unknown model
- **WHEN** a profile names `nearai/some-new-model` that the snapshot does not list
- **THEN** the attested listing is refused

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
