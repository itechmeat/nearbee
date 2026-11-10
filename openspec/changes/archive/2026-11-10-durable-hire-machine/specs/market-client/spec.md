# Spec Delta

## MODIFIED Requirements

### Requirement: Read retries
Reads SHALL retry up to three times on transport errors and 429, 502, 503, 504, honouring `Retry-After`, within a total of 30 seconds per call (a `Retry-After` that would pass that total ends the call as a retryable read); writes SHALL never be retried except as the next two requirements allow.

#### Scenario: Read after 503
- **WHEN** a read answers 503 with `Retry-After: 2` and then succeeds
- **THEN** the client waits at least 2 seconds and returns the second answer

#### Scenario: Long Retry-After
- **WHEN** a read answers 503 with `Retry-After: 120`
- **THEN** the client returns a retryable read error at once instead of waiting

### Requirement: Decline and progress repeats
`decline` SHALL be repeated at most once; a `400 invalid_state_transition` answer to the repeat SHALL count as landed only when a fresh read of the row shows it cancelled, and otherwise SHALL be returned as refused. `progress` SHALL never be repeated by the client: a `409 progress_report_too_soon` answer SHALL be returned at once with its `retry_after` delay for the caller to schedule.

#### Scenario: Decline already landed
- **WHEN** a repeated decline answers `400 invalid_state_transition` and a fresh read shows the row cancelled
- **THEN** the client reports the decline as done

#### Scenario: Decline overtaken
- **WHEN** a repeated decline answers `400 invalid_state_transition` and a fresh read shows the row accepted
- **THEN** the client reports refused

#### Scenario: Progress too soon
- **WHEN** progress answers `409 progress_report_too_soon` with `retry_after` 12
- **THEN** the client returns the 12-second delay at once, without waiting or repeating

## ADDED Requirements

### Requirement: Row fields
The client SHALL map into each row, besides the S2 fields: `createdAt`, `updatedAt`, `finalizedAt`, `slaSeconds`, `escrowAmount` (decimal), `escrowToken`, `jobId`, the job's `title`, the job's attachments and the latest message's attachments (id, filename, content type, byte size, sha256, download path), and the latest message's `senderSide`, `createdAt` and `truncated`.

#### Scenario: Contract
- **WHEN** a schema-built `MyAssignmentItem` with attachments is parsed
- **THEN** every listed field reaches the row, checked by the contract test against the OpenAPI

### Requirement: Thread read
The client SHALL list an assignment's messages (`GET /v1/assignments/{id}/messages`, newest first, paged with `limit` and `offset`) and return each message's id, body, `senderAgentId`, attachments and creation time, stopping at the first page that holds a message created at or before a caller-given time.

#### Scenario: Full body of a truncated message
- **WHEN** the caller asks for the message created at the latest message's `createdAt`
- **THEN** the client returns that message with its full body, built from `ListMessagesResponse` as the OpenAPI defines it

### Requirement: Attachment download
The client SHALL download an attachment by streaming its bytes into a caller-given writer, sending the bearer only to the configured origin, following no redirect, stopping after a caller-given byte cap, and SHALL report 503 with its `Retry-After` as a retryable read, 403 and 404 as refused.

#### Scenario: Redirect
- **WHEN** the download answers 302 to another host
- **THEN** the client does not follow it and returns a protocol error

### Requirement: Agent writes kept for an owner token
The client's agent patch and pricing plan creation SHALL stay available behind the `Market` trait for a later owner sign-in, and SHALL not be called by any command that holds only an agent token.

#### Scenario: List command
- **WHEN** `nearbee list` runs
- **THEN** neither the agent patch nor the plan creation is called
