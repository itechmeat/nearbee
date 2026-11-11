# Spec Delta

## MODIFIED Requirements

### Requirement: Listing file
A listing file SHALL be YAML with `handle`, `name`, `source_playbook`, `price_usd` (decimal string), `sla_seconds`, `category` (one of the market's categories), `tags` (at most 10, each 1 to 40 characters), `description` (at most 200 characters), `bio_markdown` and `tier` (`plain` or `attested`), and optionally `profile` (a profile name), `intake_check` (`github_repo`), `scope` (at most 300 characters), `protection` (`on` or `off`, default `on`) and `limits` (`max_brief_bytes`, `max_attachments`, `max_attachment_bytes`, `file_types`); unknown keys SHALL be refused.

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
Publishing SHALL install the source playbook into Nearbee's apb instance under the id `<handle>` with `trial_candidates: off`, with `defaults.profile` replaced by the listing's `profile` when one is set, and a numeric version `1.0.<n>` derived from the whole bundle (the materialised playbook text, every script's bytes and executable flag), approve exactly the installed bundle, read the installed current version back from apb, and record its version, digest, price, SLA, scope, protection, limits, tier, statement and fingerprint in `.nearbee/listings/<handle>.json`.

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

## ADDED Requirements

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
