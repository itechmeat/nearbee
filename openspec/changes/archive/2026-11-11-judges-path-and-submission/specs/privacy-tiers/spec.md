# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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
