# Spec Delta

## ADDED Requirements

### Requirement: Review notes in the deliverable
`nearbee audit finish` SHALL accept an optional `notes` list in `audit/candidates.json` (at most 10 strings of at most 300 characters) and SHALL render it as a "Review notes" section before "Not reported", escaped as untrusted text; without notes the deliverable SHALL be unchanged.

#### Scenario: Failed lens reported
- **WHEN** the candidates file carries the note "The tests lens failed after one send-back"
- **THEN** the deliverable shows it under "Review notes"

#### Scenario: Too many notes refused
- **WHEN** the candidates file carries 11 notes
- **THEN** `audit finish` fails and writes no `out/result.json`
