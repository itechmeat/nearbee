## ADDED Requirements

### Requirement: Awarded bids are hires
An assignment created by the award of a bid SHALL be served exactly like an instant hire: the brief is the job's title and description, the job's attachments are the hire's inputs, and the ceiling is computed from the assignment's escrow amount. Serve SHALL not distinguish the two by the assignment's `bidId` or the job kind.

#### Scenario: Board award
- **WHEN** the assignments poll returns an `in_progress` row with a `bidId`, a `standard` job and an escrow of 2.00
- **THEN** serve takes it through intake and the run like an instant hire, with a ceiling of 1.65

### Requirement: Zero-byte output files
Serve SHALL skip an output file of zero bytes that the manifest lists: it SHALL not upload it, SHALL journal the skip with the file's path once per generation, and SHALL deliver the Markdown and the other files.

#### Scenario: Empty patch
- **WHEN** a succeeded run's manifest lists an empty `docs.patch` and a non-empty `report.json`
- **THEN** serve uploads only `report.json`, journals `docs.patch` as skipped, and submits

#### Scenario: Only an empty file
- **WHEN** the manifest's only output file is empty
- **THEN** serve submits the Markdown with no file and the journal names the skipped file
