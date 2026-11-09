# hire-journal Specification

## Purpose
Fixes the record format and on-disk layout of a hire's journal so later stages can write, replay and recover hires without a migration.

## Requirements

### Requirement: Journal location
Each hire SHALL have one journal file at `<state_dir>/hires/<safe-id>/journal.jsonl`, where `<safe-id>` is derived from the market hire id using only `[A-Za-z0-9_-]` and is stable for the same id.

#### Scenario: Distinct ids stay distinct
- **WHEN** two different hire ids sanitize to the same characters (`a/b` and `a_b`)
- **THEN** their derived directory names differ

#### Scenario: Unsafe id characters
- **WHEN** a hire id contains `/` or `..`
- **THEN** the derived directory name contains neither and stays inside `<state_dir>/hires/`

### Requirement: Record envelope
Every journal line SHALL be one JSON object with `schema_version`, `seq` (consecutive from 1 within the file: the first line is 1 and each next line is the previous plus one), `timestamp` (RFC 3339 UTC), `hire_id`, `event_type` and `payload`.

#### Scenario: Round trip
- **WHEN** a record is serialized and parsed back
- **THEN** all envelope fields and the payload are equal to the original

#### Scenario: Gap in the sequence
- **WHEN** a journal's lines carry `seq` 1 and then 3
- **THEN** reading the journal fails with a sequence error naming line 2, the previous `seq` and the one found

#### Scenario: Newer schema version
- **WHEN** a line carries a `schema_version` greater than this version supports
- **THEN** reading the journal fails with an unsupported-version error naming the line, and nothing is written to that hire

#### Scenario: Unknown event type from a newer version
- **WHEN** a line carries an `event_type` this version does not know
- **THEN** parsing returns it as an unknown record with its raw payload instead of failing

### Requirement: Intents and results
An external write SHALL be represented by an intent record carrying a stable `operation_id`, the operation kind, the execution number, a digest of the request and the references needed to repeat it, and by a result record that names the same `operation_id` and keeps the external ids it obtained.

#### Scenario: Matching an intent to its result
- **WHEN** a journal holds an intent and later a result with the same `operation_id`
- **THEN** reading the journal pairs them and reports no open intent for that operation

#### Scenario: Open intent
- **WHEN** a journal ends with an intent that has no result
- **THEN** reading the journal reports that operation as open

### Requirement: No sensitive content in the journal
Journal records SHALL NOT contain the buyer's brief text, attachment contents, secrets or model transcripts; they MAY contain digests, sizes, ids and file names.

#### Scenario: Received record
- **WHEN** a received record is built from a hire with a brief
- **THEN** the record holds the brief's SHA-256 and byte length but not its text
