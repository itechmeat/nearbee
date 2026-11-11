# listing-publish Specification

## Purpose
Defines how a listing file becomes a draft agent on the market backed by a materialised apb playbook.

## Requirements

### Requirement: Listing file
A listing file SHALL be YAML with `handle`, `name`, `source_playbook`, `price_usd` (decimal string), `sla_seconds`, `category`, `tags` (at most 10, 1 to 40 characters), `description` (at most 200), `bio_markdown` and `tier` (`plain`, `attested`), and optionally `profile`, `intake_check` (`github_repo`), `scope` (at most 300), `protection` (`on` by default, or `off`) and `limits` (`max_brief_bytes`, `max_attachments`, `max_attachment_bytes`, `file_types`); unknown keys SHALL be refused.

#### Scenario: Category outside the enum
- **WHEN** `category` is `games`
- **THEN** loading fails naming the field and the allowed values

#### Scenario: Price as a float
- **WHEN** `price_usd` is the YAML number `0.5` instead of a string
- **THEN** loading fails asking for a decimal string

#### Scenario: Unknown limit
- **WHEN** `limits` contains `max_pages`
- **THEN** loading fails naming the key

#### Scenario: Price too low for protection
- **WHEN** `price_usd` is `0.26` and `protection` is not `off` (`0.26 * 0.95 - 0.25` is not above zero)
- **THEN** loading fails saying the per-hire ceiling would not be above zero, while `0.264` loads

#### Scenario: Tier missing
- **WHEN** a listing file has no `tier`
- **THEN** loading fails naming the field and the allowed values

### Requirement: Materialisation
Publishing SHALL install the source playbook into Nearbee's apb instance as `<handle>` with `trial_candidates: off`, `defaults.profile` set to the listing's `profile` when given, and a version `1.0.<n>` derived from the whole bundle (materialised text, every script's bytes and executable flag); approve exactly that bundle, read the current version back, and record version, digest, price, SLA, scope, protection, limits, tier, statement and fingerprint in `.nearbee/listings/<handle>.json`.

#### Scenario: Republish unchanged
- **WHEN** the source playbook has not changed since the last publish
- **THEN** the installed version and digest stay the same

#### Scenario: Market call fails after the install
- **WHEN** apb installed a new version and the pricing plan read that follows fails
- **THEN** `.nearbee/listings/<handle>.json` already names the installed version and digest (it is written before the first market call), the command exits non-zero, and serve can serve the installed version

#### Scenario: Script-only change
- **WHEN** only a script's bytes changed since the last publish
- **THEN** the derived version changes and the new bundle installs

#### Scenario: Scripts await approval
- **WHEN** apb's import answers that the version has scripts and asks for approval with `--digest sha256:<d>`
- **THEN** publishing approves exactly that digest and reads the version back

#### Scenario: Two variants from one source
- **WHEN** listings `repo-audit-fast` (profile `nearbee-near`) and `repo-audit-premium` (profile `nearbee-own-premium`) name the same source
- **THEN** apb holds two playbooks whose `defaults.profile` are those names, with different versions

#### Scenario: Node binds a profile
- **WHEN** a listing sets `profile` and a node of the source names its own profile
- **THEN** publishing fails naming the node before installing

### Requirement: Trial setting
Publishing SHALL refuse, before any market call, a source playbook that sets `trial_candidates` to any value other than `off`, and an installed current version that does not read back with `trial_candidates: off`.

#### Scenario: Source sets another trial value
- **WHEN** the source playbook declares a `trial_candidates` value other than `off`
- **THEN** publishing fails naming the setting before any market call

#### Scenario: Installed version lacks the trial setting
- **WHEN** the installed current version read back from apb does not carry `trial_candidates: off`
- **THEN** publishing fails before any market call

### Requirement: No market writes
Publishing SHALL send no market write: no agent patch, no pricing plan creation, no make-default, and never `listing_status`.

#### Scenario: Install only
- **WHEN** a listing is published
- **THEN** the market receives only reads

### Requirement: Web-app settings report
After installing, publishing SHALL read the agent's pricing plans and print, for the owner, the values to set in the web app (name, description, bio, category, tags, SLA, price) and per value whether the market matches: the default plan's model is `per_call` and its price equals `price_usd`; its SLA equals `sla_seconds`, or is `null` and printed as unverified (it inherits the agent default, which the token cannot read). A mismatch SHALL be printed and SHALL not fail the command.

#### Scenario: No plan yet
- **WHEN** the agent has no pricing plan
- **THEN** publishing succeeds, says "installed", and prints that the price is not set in the web app yet

#### Scenario: Price differs
- **WHEN** the default plan's price is `1.00` and the file says `0.50`
- **THEN** the report marks the price as a mismatch and the command exits 0

### Requirement: Tier check at publish
Publishing SHALL run the tier check (spec `privacy-tiers`) on the facts of the materialised source before installing and on the facts read back after installing, and SHALL write the listing record only when both pass and their fingerprints are equal.

#### Scenario: Source breaks the tier
- **WHEN** an attested listing's profile falls back to an incognito model
- **THEN** publishing fails before installing and names the model

### Requirement: Statement in the settings report
The web-app settings report SHALL print the generated data statement as the block to paste at the end of the bio, and SHALL print the tier.

#### Scenario: Report of an attested listing
- **WHEN** an attested listing is published
- **THEN** the report shows `tier: attested` and the `Data handling` block
