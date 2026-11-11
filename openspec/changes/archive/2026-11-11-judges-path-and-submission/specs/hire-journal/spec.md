# Spec Delta

## ADDED Requirements

### Requirement: Record time from the serve clock
Every journal record SHALL be stamped with the time of the clock the writer was opened with: serve passes its round clock, every other writer the real clock. The record format SHALL be unchanged.

#### Scenario: Injected clock
- **WHEN** a writer opened with a clock fixed at 2026-11-10T00:00:00Z appends a record
- **THEN** the record's timestamp is 2026-11-10T00:00:00Z

#### Scenario: Deadline from the injected clock
- **WHEN** serve opens a redo generation under a clock and the clock is advanced past the redo deadline
- **THEN** serve treats the redo deadline as reached
