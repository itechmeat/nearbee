# data-statement Specification

## Purpose
Defines the data statement Nearbee generates for a listing from its checked execution facts, its fingerprint, and where the statement appears, so the buyer reads what the adapter checked and not what someone typed.

## Requirements

### Requirement: Generated content
The statement SHALL be Markdown generated only from the listing's tier and execution facts, under the heading `Data handling`, and SHALL name each executor and fallback with its model and where it runs (the owner's subscription at a named provider, or NEAR AI Cloud Confidential TEE), every connector binding, and whether decisions and compaction are on or off.

#### Scenario: Plain listing
- **WHEN** the statement of a plain listing on `nearbee-own-premium` is generated
- **THEN** it names the `claude` agent, the model and the owner's subscription, and makes no TEE claim

#### Scenario: Plain listing with channels on
- **WHEN** a plain listing binds a connector and the project's decisions are on
- **THEN** the statement names the connector, says decisions are on, and says those channels may send data to providers Nearbee has not validated

### Requirement: Where the run happens
The statement SHALL say that the run happens on the owner's self-hosted machine, which can read the data. Without a sandbox fact it SHALL say that the steps run without a sandbox and that reading files the owner's account can read and network access the bindings do not declare are not prevented. With a sandbox fact it SHALL instead describe the sandbox as the next requirement says.

#### Scenario: Attested listing
- **WHEN** the statement of an attested listing is generated
- **THEN** it still says the owner's machine can read the data

#### Scenario: No sandbox fact
- **WHEN** the facts carry no sandbox fact
- **THEN** the section text is byte for byte the text before this change

### Requirement: Sandbox wording
With a sandbox fact the statement SHALL say that the steps run in a container on the owner's machine with a read-only system, no added privileges and only the instance's own directories mounted, that outbound connections are limited to the listed hosts, and that hires of every listing on that machine share the container and are kept apart by working directories only.

#### Scenario: Sandboxed instance
- **WHEN** the facts carry a sandbox fact with the hosts `cloud-api.near.ai` and `github.com`
- **THEN** the statement names the container limits, lists exactly those two hosts in sorted order, says hires share the container, and does not contain "run without a sandbox"

### Requirement: Market and retention wording
The statement SHALL say that the brief, thread messages and uploaded files stay with the market under its policy and that a file once used by a job cannot be deleted there; and that Nearbee deletes its own copies (the hire's worktree and snapshots) a configured number of days after acceptance, while apb run directories, decision logs and executor caches are not deleted by Nearbee.

#### Scenario: Retention days
- **WHEN** `retention.after_acceptance_days` is 7
- **THEN** the statement says 7 days and names what cleanup does not cover

### Requirement: Deterministic text
The statement SHALL contain no timestamps, prices, run ids or machine paths, and SHALL list facts in a fixed order, so the same facts always give the same bytes. An unknown fact SHALL be stated as `unknown` without its reason, since a reason may name a path.

#### Scenario: Same facts twice
- **WHEN** the statement is generated twice from equal facts
- **THEN** both texts are byte-identical

#### Scenario: Unknown setting with a path in its reason
- **WHEN** the decisions setting is unknown because a file under the owner's home is missing
- **THEN** the statement says "Decisions are unknown." and names no path

### Requirement: Fingerprint and record
Publishing SHALL record the tier, the statement text, its SHA-256, the statement format version and a fingerprint (SHA-256 of a canonical encoding of the tier and the facts, keeping fallback and tier order). A fact change that leaves the text unchanged SHALL change the fingerprint.

#### Scenario: Fallback changed, text equal
- **WHEN** two TEE fallbacks swap order without changing the statement text
- **THEN** the fingerprint differs

### Requirement: Drift
A listing SHALL be drifted when its current fingerprint differs from the recorded one, when the statement rendered from current facts and retention settings differs from the recorded text, or when its record lacks a tier, fingerprint, statement, matching statement hash or supported statement format.

#### Scenario: Retention changed
- **WHEN** `retention.after_acceptance_days` changes from 7 to 30 after publishing
- **THEN** the listing is drifted until it is listed again

#### Scenario: Old record
- **WHEN** serve reads an S3 listing record with no tier
- **THEN** the listing is drifted and the operator is told to run `nearbee list` again

### Requirement: Deliverable footer
Every deliverable SHALL end with the statement the hire's generation snapshotted at its first apb start, separated from the playbook's Markdown by a horizontal rule.

#### Scenario: Republished mid-hire
- **WHEN** the listing is listed again with a new statement while a hire's generation 1 runs
- **THEN** that generation's deliverable carries the statement snapshotted at its start

### Requirement: Direct models in the statement
The statement SHALL list each direct model as called directly by Nearbee commands in script steps, with where it runs, and SHALL name the signed step when the listing's `signed_models` is not empty; the fingerprint SHALL cover the direct and signed models in order and SHALL be unchanged for a listing without them.

#### Scenario: Agent B statement
- **WHEN** the statement of `doc-digest-private` is generated
- **THEN** it names Qwen3-VL, the privacy filter and DeepSeek as direct NEAR AI Cloud TEE calls and says the DeepSeek call carries a response signature

#### Scenario: No drift from direct models
- **WHEN** the drift pass inspects a listing with direct models whose apb facts are unchanged
- **THEN** the listing is clean

#### Scenario: S4 fingerprint unchanged
- **WHEN** a listing without direct models is fingerprinted
- **THEN** the fingerprint equals the one S4 computed for the same facts

### Requirement: Script models in the statement
When the listing declares `script_models`, the statement SHALL contain a "Script model calls" section saying the script steps call these models through the owner's agent CLIs as declared by the owner and not observed by Nearbee, and SHALL name each entry with the same per-agent wording as the Models section; the fingerprint SHALL cover the list in order. Without the field the statement SHALL be unchanged.

#### Scenario: ux-flow statement
- **WHEN** the statement of a plain listing with `script_models: [claude/claude-opus-5-5]` is generated
- **THEN** it says the scripts call `claude`, model `claude-opus-5-5`, the owner's Claude subscription (Anthropic), declared by the owner

#### Scenario: DeepSeek Harness reviewer
- **WHEN** a plain listing declares `dsh/zai/glm-5.3-flash`
- **THEN** the statement names `dsh` (DeepSeek Harness), model `zai/glm-5.3-flash`, through the owner's own provider account `zai`

#### Scenario: No script models
- **WHEN** a listing without `script_models` gets its statement
- **THEN** the text is byte for byte the text before this change
