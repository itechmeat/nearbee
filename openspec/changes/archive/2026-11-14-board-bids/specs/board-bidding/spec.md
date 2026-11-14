# board-bidding Specification

## Purpose

Lets a served listing find jobs on the market's public job board, bid on the ones it can do with a proposal generated from the listing, and track each bid to its award, so an award reaches the normal hire path.

## ADDED Requirements

### Requirement: Bidder cadence
While `nearbee serve` runs and at least one served listing has a board block, serve SHALL run one bidder pass per board interval, never more often than its poll interval, after the hire round of the same loop iteration. A listing without a board block or without credentials SHALL never bid.

#### Scenario: Interval respected
- **WHEN** the board interval is 300 s and the poll interval is 45 s
- **THEN** the bidder lists the board at most once per 300 s per listing, and every hire round still runs every 45 s

#### Scenario: No board block
- **WHEN** no listing in the config has a board block
- **THEN** serve makes no board read and no bid

### Requirement: Pass time budget
One bidder pass over every listing SHALL stop when the board block's `pass_budget_secs` is spent: no further read, scope question or bid is started, a scope question waits at most the time left, and each listing not reached is logged as skipped with `time_budget_spent`. The rest waits for the next pass, so the next hire round (awards, the first-hour decline window) is not held back.

#### Scenario: Budget spent
- **WHEN** the pass budget is spent before a listing's turn
- **THEN** that listing makes no market read and places no bid this pass, and the log names `time_budget_spent`

#### Scenario: Slow scope answer
- **WHEN** the scope question takes longer than the time left in the pass
- **THEN** the pass stops waiting at the budget and places no bid on that job

### Requirement: Job choice
For each listing with a board block the bidder SHALL list the board with the block's query and each configured kind, and SHALL consider only a job whose kind is configured and not `competition`, whose status is `open` or `filling` with a free slot, that is not a test run or a quote request, that does not require verifiable delivery (a Nearbee bid does not offer it), and on which the agent holds no pending bid.

#### Scenario: Verifiable delivery required
- **WHEN** a job has `requiresVerifiable` set
- **THEN** no bid is placed and the skip names `verifiable_required`

#### Scenario: Kind not configured
- **WHEN** the block's kinds are `[standard]` and an `instant` job is listed
- **THEN** no bid is placed and the skip names `kind_not_configured`

### Requirement: Job terms
The bidder SHALL bid only on a job whose budget token is `USDC`, whose budget, when the job names one, is at or above the bid amount, whose required builder verification the owner's configured level meets, and whose poster is in the allowlist when one is configured.

#### Scenario: Budget below the amount
- **WHEN** a job names a budget of 1.00 USDC and the bid amount is 2.00
- **THEN** no bid is placed and serve logs the job as skipped with the reason `budget_below_amount`

#### Scenario: No budget named
- **WHEN** a matching job has no budget
- **THEN** the budget does not block the bid

#### Scenario: Verification above the owner's level
- **WHEN** a job asks for `verified` and the configured owner level is `none`
- **THEN** no bid is placed and the skip names `verification_required`

#### Scenario: Poster not allowed
- **WHEN** the block lists a poster allowlist and the job's creator account is not in it
- **THEN** no bid is placed and the skip names `poster_not_allowed`

#### Scenario: USD job
- **WHEN** a matching job's budget token is `USD`
- **THEN** no bid is placed and the skip names `usd_job`

### Requirement: Intake before a bid
Before a bid the bidder SHALL run the listing's intake checks on the job, with the job's title and description as the brief and the bid amount as the escrow: the daily loss limit, the hard limits, the cost admission, the listing's deterministic intake check and, with protection on and a scope, the scope check. A job that a check declines SHALL get no bid; a scope check without an answer SHALL not block it, as at intake.

#### Scenario: Out of scope
- **WHEN** the scope check answers a clear no for a job
- **THEN** no bid is placed and the skip names the `scope` check

#### Scenario: Hard limit
- **WHEN** the job's brief is longer than the listing's `brief_bytes` limit
- **THEN** no bid is placed and the skip names the `hard_limits` check

#### Scenario: Asked once
- **WHEN** a job was declined by the scope check and is listed again unchanged on the next pass
- **THEN** the scope question is not asked again for it while serve runs

### Requirement: Job attachments at intake
The intake before a bid SHALL see the job's attachments as `GET /v1/jobs/{job_id}/attachments` lists them, so the hard limits on attachment count and size judge the bid as they will judge the award. A job whose attachments cannot be read SHALL get no bid this pass and SHALL be logged with `attachments_unreadable`.

#### Scenario: Attachments over the limit
- **WHEN** a job carries more attachments than the listing's `max_attachments`
- **THEN** no bid is placed and the skip names the `hard_limits` check

#### Scenario: Attachments unreadable
- **WHEN** the attachment read of a job is refused
- **THEN** no bid is placed this pass and the skip names `attachments_unreadable`

### Requirement: Scope answers kept
A scope answer that passes SHALL be kept for the job, while its `updatedAt` is unchanged and serve runs, so a job held back after the answer (the journal could not be written, the time budget) is not asked again.

#### Scenario: Held back after a yes
- **WHEN** a job passed the scope check and its intent could not be journaled
- **THEN** the next pass bids on it without asking the scope question again

### Requirement: Generated proposal
The proposal SHALL be built by code from the listing record alone: what the listing delivers (its scope), how (a playbook run on the owner's machine and the privacy tier), the models the listing declares, the promised delivery window, and a pointer to the data statement delivered with the work. It SHALL contain no text of the job and SHALL make no model call.

#### Scenario: No buyer text
- **WHEN** a bid is placed on a job whose description contains a distinctive phrase
- **THEN** the proposal sent does not contain that phrase and equals the proposal built for the same listing on any other job

#### Scenario: Models named
- **WHEN** the listing declares the script model `claude/claude-sonnet-5-5`
- **THEN** the proposal names `claude-sonnet-5-5` through `claude`

### Requirement: Idempotent journaled bids
Every bid SHALL be preceded by a durable intent in the bid journal at `<state_dir>/bids/<handle>/<job id>.jsonl` and followed by its result. A job with any journal entry SHALL never get a new bid. An intent left open by a crash SHALL close as placed when a pending bid or an assignment is on its job, else it is resent once, where `409 duplicate_bid` counts as placed. A refused bid SHALL be recorded with its code and never retried.

#### Scenario: Restart after the bid
- **WHEN** serve crashes after the bid landed and before its result was journaled, and restarts
- **THEN** it reads the agent's pending bids, finds the bid on that job, closes the intent as placed and sends no second bid

#### Scenario: Refused bid
- **WHEN** the market answers `409 cluster_already_in_job`
- **THEN** the journal records `refused` with that code and the job gets no further bid

#### Scenario: Bid already on the market
- **WHEN** the agent's pending bids already hold a bid on a job that has no journal entry
- **THEN** no bid is placed and the skip names `already_bid`

### Requirement: Throttled bids
A throttled answer to a bid (`408`, `429`, or another 4xx naming `Retry-After`) SHALL not be a refusal: the intent stays open, counts as an open bid, and the bid is sent again on a later pass.

#### Scenario: Throttled bid
- **WHEN** the market answers a bid with `429`
- **THEN** the journal keeps the intent open, records no refusal, and the next pass sends the bid again

### Requirement: Open bid cap
The bidder SHALL count the listing's open bids, the agent's market bids with status `pending` plus its journal intents still open (a resent intent stays counted unless the market refuses it), and SHALL place no bid while that count is at `max_open_bids`. The cap SHALL be checked before the attachment read, the intake and the scope question, so a full listing asks no question.

#### Scenario: Resent intent counted
- **WHEN** `max_open_bids` is 3, the agent has 2 pending bids and an open intent is resent and placed
- **THEN** no other bid is placed in the same pass

#### Scenario: Cap reached
- **WHEN** `max_open_bids` is 3 and the agent has 3 pending bids
- **THEN** no bid is placed this pass, no scope question is asked, and each matching job is logged as skipped with `max_open_bids`

### Requirement: Bid outcomes
Each pass SHALL read only the agent's pending bids (never every status, which grows with each instant hire) and the listing's assignment job ids from the same loop iteration's hire read. A journaled bid reads `pending` while pending, `accepted` once an assignment on its job appears, and `closed` when neither holds. Each change SHALL be journaled; a change to `accepted` SHALL be logged as awarded. The bidder SHALL make no other write for an award.

#### Scenario: Awarded
- **WHEN** a journaled bid is no longer pending and an assignment on its job appears
- **THEN** the journal gains a status record `accepted` once and serve logs the award with the job and the listing

#### Scenario: Lost bid
- **WHEN** a journaled bid is no longer pending and no assignment on its job appears
- **THEN** the journal gains a status record `closed` once (rejected, withdrawn or deleted: the agent token cannot tell them apart)

#### Scenario: Hire read failed
- **WHEN** the assignment read of the loop iteration failed and a journaled bid is not pending
- **THEN** its journal is left as it is until a later pass

### Requirement: Bid decisions logged
Serve SHALL log one line per bid decision: matched, skipped with its reason, bid sent with the amount, refused with its code, and awarded. A line SHALL name the listing and the job id and SHALL never contain the job's description or the proposal text.

#### Scenario: Skip logged
- **WHEN** a job is skipped because its kind is not configured
- **THEN** serve prints a line naming the listing, the job id and `kind_not_configured`

### Requirement: Bids listing
`nearbee bids [--json]` SHALL list every journaled bid, read only and without the serve lock: the listing, the job id, the amount, the SLA, the state (`placing`, `placed`, `refused` with its code, or the last market status read) and the time of the latest record. It SHALL print no job text and no proposal text.

#### Scenario: Listing
- **WHEN** the journal holds one placed bid and one refused bid
- **THEN** `nearbee bids` prints two lines with those states, and `--json` prints the same as a JSON array
