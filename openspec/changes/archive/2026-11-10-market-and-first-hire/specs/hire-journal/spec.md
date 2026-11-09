# Spec Delta

## ADDED Requirements

### Requirement: Journal writing
Records SHALL be appended one line at a time to the hire's journal file with the next consecutive `seq`, the file and its directory SHALL be flushed to disk (fsync) before the external write a record announces, and a hire's journal SHALL have one writer at a time within a process.

#### Scenario: Intent is durable before the write
- **WHEN** serve is about to start an apb run
- **THEN** the run intent is on disk, flushed, before the start request is sent

#### Scenario: Append after restart
- **WHEN** a journal with records 1 to 3 is reopened
- **THEN** the next record gets `seq` 4

#### Scenario: Truncated tail
- **WHEN** the last line is truncated by a crash
- **THEN** reopening for writing fails and reports the hire for the operator instead of appending after a broken line
