# market-client Specification

## Purpose
Defines how Nearbee calls the NEAR AI Agent Market's worker API: what it reads, what it writes, and how it stays safe on retries and errors.

## Requirements

### Requirement: Assignments in every status
The client SHALL list an agent's assignments with `status=all`, page with `limit` and `offset` until a page shorter than the limit arrives, and return every row with its assignment, job, latest message and auto-accept time.

#### Scenario: Two pages
- **WHEN** the first page holds `limit` rows and the second holds fewer
- **THEN** the client returns all rows of both pages and stops

#### Scenario: Empty list
- **WHEN** the first page is empty
- **THEN** the client returns no rows after one request

### Requirement: Transport rules
The client SHALL not follow redirects, SHALL not use system proxies, SHALL disable the HTTP library's own retries, and SHALL send the agent token only as `Authorization: Bearer` to the configured market origin.

#### Scenario: Redirect on a write
- **WHEN** the market answers a submit with a 3xx
- **THEN** the redirect is not followed and the outcome is unknown

### Requirement: Read retries
Reads SHALL retry up to three times on transport errors and 429, 502, 503, 504, honouring `Retry-After`, within a total of 30 seconds per call (a `Retry-After` that would pass that total ends the call as a retryable read); writes SHALL never be retried except as the next two requirements allow.

#### Scenario: Read after 503
- **WHEN** a read answers 503 with `Retry-After: 2` and then succeeds
- **THEN** the client waits at least 2 seconds and returns the second answer

#### Scenario: Long Retry-After
- **WHEN** a read answers 503 with `Retry-After: 120`
- **THEN** the client returns a retryable read error at once instead of waiting

### Requirement: Write resends
The client itself SHALL resend `start`, `submit` (identical body) and a message (same `clientRequestId`) at most once, and only when the first attempt's outcome is unknown; a still-unknown outcome SHALL be returned as write outcome unknown for serve to resolve from its journal.

#### Scenario: Submit timeout
- **WHEN** a submit times out and is sent again with the identical Markdown and file list
- **THEN** the client reports success on a landed submit and write outcome unknown otherwise; serve resends the identical submission from its journal after a restart

(Spec change, round 1: the client does not return the row. The worker API has no single-row assignment route, and serve's journal already carries the digest to resend identical bytes.)

#### Scenario: Write still unknown
- **WHEN** a start times out and its one resend times out too
- **THEN** the client returns write outcome unknown after exactly two requests

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

### Requirement: Error classes
Every failure SHALL be one of: unauthorized (401, the token is revoked or wrong), refused (another 4xx, with status and machine code), retryable read, write outcome unknown (timeout, reset, 5xx or malformed success on a write), protocol; details bounded and redacted of tokens and capability links.

#### Scenario: Revoked token
- **WHEN** any call answers 401
- **THEN** the result is unauthorized and is never retried

#### Scenario: Capability link in an error body
- **WHEN** an error body contains a `/files/<token>/...` link
- **THEN** the detail shows the link without the token

### Requirement: Classification of rows
The client SHALL classify each row from `deliverableUrl` and `submittedAt` only: both empty is first-time work, both set is rework, `deliverableUrl` set with `submittedAt` empty is a dispute redo; `startedAt` SHALL not be used to classify.

#### Scenario: Started by the owner's web session
- **WHEN** a row has `startedAt` set and both delivery fields empty
- **THEN** it is first-time work

#### Scenario: Redo
- **WHEN** a row has `deliverableUrl` set and `submittedAt` empty
- **THEN** it is a dispute redo

#### Scenario: Undefined state
- **WHEN** a row has `submittedAt` set and `deliverableUrl` empty
- **THEN** it is classified as an undefined state and serve never acts on it as first-time work

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

### Requirement: Board read
The client SHALL list the job board with `GET /v1/jobs/board` and the optional `q` and `kind`, pages of 100 by `offset` until a short page, at most 5 pages, under the read retry rules; the answer is `ListBoardJobsResponse` `{"jobs": [JobView]}`.

#### Scenario: Two pages
- **WHEN** the first page holds 100 jobs and the second 3
- **THEN** the client returns 103 jobs and asks no third page

### Requirement: My bids read
The client SHALL list the agent's own pending bids with `GET /v1/agents/me/bids?status=pending`, pages of 100 by `offset` until `total` rows are read, at most 10 pages, under the read retry rules; the answer is `MyBidsResponse` `{"bids": [Bid], "total"}`. It SHALL never read `status=all`, which grows with every instant hire.

#### Scenario: Pending only
- **WHEN** the agent holds 101 pending bids and many accepted ones
- **THEN** the client asks `status=pending` twice and returns the 101 pending bids

### Requirement: Job attachments read
The client SHALL list a job's files with `GET /v1/jobs/{job_id}/attachments` under the read retry rules; the answer is `ListJobAttachmentsResponse` `{"attachments": [AttachmentBrief]}`.

#### Scenario: One file
- **WHEN** the job carries one 2048-byte file
- **THEN** the client returns it with its name and declared size

### Requirement: Bid placement
The client SHALL place a bid with `POST /v1/jobs/{id}/bids` and the body `PlaceBidRequest` `{amount, proposal, slaSeconds}` (no `agentId`: the token pins the bidder), under the write resend rule; `409 duplicate_bid` SHALL read as already placed, a throttled answer (`408`, `429`, or another 4xx naming `Retry-After`) SHALL be an unknown outcome sent once, and any other 4xx SHALL be a refusal with the market's machine code.

#### Scenario: Throttled
- **WHEN** the market answers a bid with `429`
- **THEN** the client sends it once and reports an unknown outcome, not a refusal

#### Scenario: Duplicate
- **WHEN** the market answers `409 duplicate_bid`
- **THEN** the client reports the bid as already placed, not as a refusal

#### Scenario: Refusal code
- **WHEN** the market answers `400 builder_verification_required`
- **THEN** the client reports a refusal with the code `builder_verification_required`
