# Spec Delta

## ADDED Requirements

### Requirement: Generation in every record
Every record written from this version on SHALL carry the generation it belongs to (1 for first-time work); a record of a journal written before this version SHALL read as generation 1.

#### Scenario: Old journal
- **WHEN** an S2 journal without generations is read
- **THEN** every record belongs to generation 1 and the hire's state is derived as before

### Requirement: Private snapshots
The request bodies a recovery must resend SHALL be stored outside the journal under `<state_dir>/hires/<safe-id>/gen-<n>/` (directory mode 0700, files 0600, written to a temporary name, flushed and renamed) before the intent that refers to them; the journal SHALL hold their SHA-256 and size only.

#### Scenario: Submit snapshot
- **WHEN** serve is about to submit generation 2
- **THEN** the Markdown is in `gen-2/submission.md` and flushed, and the submit intent carries its digest, before the submit request is sent

#### Scenario: Directory left unsynced by a crash
- **WHEN** serve stopped after creating a hire's directory and before syncing its parent, and starts again
- **THEN** the parent is synced before any record is written under that directory, and a failed sync writes nothing

#### Scenario: Snapshot does not match
- **WHEN** a recovery finds a snapshot whose digest differs from its intent's
- **THEN** serve sends nothing for that hire and sets an attention code

### Requirement: Read errors
Reading a hire's journal SHALL treat only a missing file as "no journal"; any other read error SHALL be reported for the operator and SHALL stop all writes for that hire.

#### Scenario: Permission denied
- **WHEN** the journal file exists and cannot be read
- **THEN** serve reports the hire and starts nothing for it
