# listing-publish Specification

## Purpose
Defines how a listing file becomes a draft agent on the market backed by a materialised apb playbook.

## Requirements

### Requirement: Listing file
A listing file SHALL be YAML with `handle`, `name`, `source_playbook`, `price_usd` (decimal string), `sla_seconds`, `category` (one of the market's categories), `tags` (at most 10, each 1 to 40 characters), `description` (at most 200 characters) and `bio_markdown`; unknown keys SHALL be refused.

#### Scenario: Category outside the enum
- **WHEN** `category` is `games`
- **THEN** loading fails naming the field and the allowed values

#### Scenario: Price as a float
- **WHEN** `price_usd` is the YAML number `0.5` instead of a string
- **THEN** loading fails asking for a decimal string

### Requirement: Materialisation
Publishing SHALL install the source playbook into Nearbee's apb instance under the id `<handle>` with fixed profile names and `trial_candidates: off`, approve exactly the installed bundle, read the installed current version back from apb, and record its version and digest in `.nearbee/listings/<handle>.json`.

#### Scenario: Republish unchanged
- **WHEN** the source playbook has not changed since the last publish
- **THEN** the installed version and digest stay the same

#### Scenario: Market call fails after the install
- **WHEN** apb installed a new version and a later market call fails
- **THEN** `.nearbee/listings/<handle>.json` already names the installed version and digest (it is written before the first market call), and the error tells the operator to publish again to finish the market update

### Requirement: Trial setting
Publishing SHALL refuse, before any market call, a source playbook that sets `trial_candidates` to any value other than `off`, and an installed current version that does not read back with `trial_candidates: off`.

#### Scenario: Source sets another trial value
- **WHEN** the source playbook declares a `trial_candidates` value other than `off`
- **THEN** publishing fails naming the setting before any market call

#### Scenario: Installed version lacks the trial setting
- **WHEN** the installed current version read back from apb does not carry `trial_candidates: off`
- **THEN** publishing fails before any market call

### Requirement: Market update
Publishing SHALL patch the agent's name, description, bio, category, tags and SLA, create a per-call pricing plan with the listing's price and SLA and make it the default, and SHALL never change `listing_status`.

#### Scenario: Status untouched
- **WHEN** a listing is published
- **THEN** no request carries `listing_status`

#### Scenario: Price unchanged
- **WHEN** the default plan already has the same price and SLA
- **THEN** no new plan is created

#### Scenario: Pricing plan cap
- **WHEN** creating a plan answers `409` because the agent already has the maximum number of plans
- **THEN** publishing fails naming the cap and tells the operator to remove an unused plan; no plan is deleted or patched
