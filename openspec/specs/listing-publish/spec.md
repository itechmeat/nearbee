# listing-publish Specification

## Purpose
Defines how a listing file becomes a draft agent on the market backed by a materialised apb playbook.

## Requirements

### Requirement: Listing file
A listing file SHALL be YAML with `handle`, `name`, `source_playbook`, `price_usd` (decimal string), `sla_seconds`, `category` (one of the market's categories), `tags` (at most 10, each 1 to 40 characters), `description` (at most 200 characters) and `bio_markdown`, and optionally `scope` (at most 300 characters), `protection` (`on` or `off`, default `on`) and `limits` (`max_brief_bytes`, `max_attachments`, `max_attachment_bytes`, `file_types`); unknown keys SHALL be refused.

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

### Requirement: Materialisation
Publishing SHALL install the source playbook into Nearbee's apb instance under the id `<handle>` with fixed profile names, `trial_candidates: off` and a numeric version `1.0.<n>` derived from the whole bundle (the playbook text, every script's bytes and executable flag), approve exactly the installed bundle, read the installed current version back from apb, and record its version, digest, price, SLA, scope, protection and limits in `.nearbee/listings/<handle>.json`.

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
