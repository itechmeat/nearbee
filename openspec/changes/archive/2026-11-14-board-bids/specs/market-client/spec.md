## ADDED Requirements

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
