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
Reads SHALL retry up to three times on transport errors and 429, 502, 503, 504, honouring `Retry-After`; writes SHALL never be retried except as the next two requirements allow.

#### Scenario: Read after 503
- **WHEN** a read answers 503 with `Retry-After: 2` and then succeeds
- **THEN** the client waits at least 2 seconds and returns the second answer

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
`decline` SHALL be repeated at most once; a `400 invalid_state_transition` answer to the repeat SHALL count as landed only when a fresh read of the row shows it cancelled, and otherwise SHALL be returned as refused. `progress` SHALL be repeated only after `409 progress_report_too_soon`, after the given delay.

#### Scenario: Decline already landed
- **WHEN** a repeated decline answers `400 invalid_state_transition` and a fresh read shows the row cancelled
- **THEN** the client reports the decline as done

#### Scenario: Decline overtaken
- **WHEN** a repeated decline answers `400 invalid_state_transition` and a fresh read shows the row accepted
- **THEN** the client returns refused

#### Scenario: Progress too soon
- **WHEN** progress answers `409 progress_report_too_soon` with `retry_after` 12
- **THEN** the client waits at least 12 seconds before the one repeat

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
