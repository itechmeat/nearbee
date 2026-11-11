# Spec Delta

## MODIFIED Requirements

### Requirement: Listing file
A listing file SHALL be YAML with `handle`, `name`, `source_playbook`, `price_usd` (decimal string), `sla_seconds`, `category`, `tags` (at most 10, 1 to 40 characters), `description` (at most 200), `bio_markdown` and `tier` (`plain`, `attested`), and optionally `profile`, `direct_models`, `signed_models`, `intake_check` (`github_repo`), `scope` (at most 300), `protection` (`on` by default, or `off`) and `limits` (`max_brief_bytes`, `max_attachments`, `max_attachment_bytes`, `file_types`); unknown keys SHALL be refused.

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

## ADDED Requirements

### Requirement: Direct and signed models
`direct_models` SHALL be a list of bare catalog ids (no `nearai/` prefix) kept in order; a prefixed or duplicated entry SHALL be refused naming it. `signed_models` SHALL be a subset of `direct_models`; any other entry SHALL be refused naming it. Both are recorded with the listing and frozen with each generation's statement snapshot.

#### Scenario: Direct models listed
- **WHEN** a listing sets `direct_models: [Qwen/Qwen3-VL-30B-A3B-Instruct, openai/privacy-filter]`
- **THEN** loading keeps the list in that order

#### Scenario: Duplicate direct model
- **WHEN** a listing lists `openai/privacy-filter` twice in `direct_models`
- **THEN** loading fails naming the duplicated entry

#### Scenario: Signed model not direct
- **WHEN** `signed_models` names a model absent from `direct_models`
- **THEN** loading fails naming it
