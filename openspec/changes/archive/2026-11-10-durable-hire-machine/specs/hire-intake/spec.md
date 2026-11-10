# Spec Delta

## Purpose

Decides, before any work starts, whether Nearbee takes a hire: the listing's hard limits, a scope check, the cost admission and the daily loss limit, with short decline texts sent within the first hour.

## ADDED Requirements

### Requirement: Order of checks
Intake SHALL run for a new first-time row before the market start, in this order: daily loss limit, hard limits, cost admission, scope check; the first check that declines SHALL decide, and every check's outcome (passed, declined, skipped with its reason) SHALL be journaled without the brief text.

#### Scenario: Hard limit before the scope check
- **WHEN** a brief exceeds the listing's `max_brief_bytes`
- **THEN** the hire is declined without asking the decision provider

#### Scenario: Journal of a taken hire
- **WHEN** a hire passes every check
- **THEN** the journal holds one intake record per check, none of them containing the brief text

### Requirement: Hard limits
When the listing file sets `limits`, intake SHALL decline `rejected` a hire whose brief (title plus description, UTF-8 bytes) is longer than `max_brief_bytes`, whose job has more attachments than `max_attachments`, whose attachments' declared sizes sum above `max_attachment_bytes`, or that has an attachment whose file extension is not in `file_types`; each limit absent means no limit.

#### Scenario: Oversized attachments
- **WHEN** the listing allows 10 MB in total and the job's attachments declare 40 MB
- **THEN** the hire is declined `rejected` with a reason naming the limit, before any download

#### Scenario: No limits
- **WHEN** the listing file has no `limits`
- **THEN** no hard limit declines the hire

### Requirement: Scope check
Unless the listing sets `protection: off`, intake SHALL ask `apb decide is` on Nearbee's apb instance, outside any run, whether the brief is a request for the listing's `scope` (its `description` when `scope` is absent), and SHALL decline `rejected` only on a clear no: an answer with probability `p` below 0.2; an answer with `p` from 0.2 to 0.5 is taken as uncertain.

#### Scenario: Clear no
- **WHEN** the decision provider answers no with `p` 0.05 to "is this a request for an audit of a public repository"
- **THEN** the hire is declined `rejected` with the listing's out-of-scope text

#### Scenario: Uncertain no
- **WHEN** the provider answers no with `p` 0.35
- **THEN** the hire is taken and the intake record says uncertain

#### Scenario: No provider configured
- **WHEN** apb answers `refused: no_provider`
- **THEN** the check is journaled as skipped (`no_provider`), the hire goes on, and the operator is told once per serve process

#### Scenario: Brief passed without a shell
- **WHEN** the brief contains `$(rm -rf ~)` and quotes
- **THEN** the decide process receives it as one argument, no shell interprets it, and nothing runs

### Requirement: Unanswered scope check
A scope question that gets no answer (no provider, a refusal, an error, a 60-second timeout) SHALL be journaled as skipped with its reason and SHALL not decline the hire; a check interrupted by a crash (its intent open after a restart) SHALL be journaled as skipped (`interrupted`) and SHALL not be asked again.

#### Scenario: Crash during the check
- **WHEN** serve dies after starting `apb decide` and before recording its answer
- **THEN** after the restart the check is skipped as `interrupted` and no second question is sent

#### Scenario: Provider error
- **WHEN** apb answers `{"answered":false,"error":"..."}`
- **THEN** the check is journaled as skipped with the reason `error` and the hire goes on

### Requirement: Cost admission
Unless the listing sets `protection: off`, intake SHALL compute the hire's ceiling as `amount * 0.95 - 0.25` USD, where `amount` is the row's `escrowAmount` when above zero and the listing's `price_usd` when the escrow is zero (an owner test run), and SHALL decline `rejected` a hire whose ceiling is not above zero.

#### Scenario: Paid hire
- **WHEN** the row's `escrowAmount` is `3.00`
- **THEN** the hire's ceiling is USD 2.60, whatever the listing file's price says

#### Scenario: Owner test run
- **WHEN** the row's `escrowAmount` is `0` and the listing's price is `0.50`
- **THEN** the hire's ceiling is USD 0.225

### Requirement: Daily loss limit
Intake SHALL sum the cost (a missing or incomplete reported cost counting as the run's whole budget) of every execution that ended without a delivery (declined, stopped or cancelled) during the current UTC day, across all hires, and SHALL decline `rejected` every new first-time hire while that sum is at or above `limits.daily_loss_usd`, telling the operator once per day.

#### Scenario: Limit reached
- **WHEN** failed executions today reported USD 21 and the limit is 20
- **THEN** a new hire is declined `rejected` with the "not taking work right now" text and no run starts

#### Scenario: End recorded before the abort is confirmed
- **WHEN** two ambiguous matches started with a USD 2.60 budget report no cost, and the hire lookup records their end before their aborts are confirmed
- **THEN** each counts USD 2.60

#### Scenario: Next day
- **WHEN** the UTC day changes
- **THEN** the sum starts again from zero

### Requirement: Decline texts and the first hour
A decline at intake SHALL use `rejected` with a short fixed text per check that names no path, apb internal or model, and states that the buyer is refunded in full. The journal SHALL record whether the decline fell within one hour of the row's `createdAt`.

#### Scenario: Late intake
- **WHEN** serve was down and declines a hire 90 minutes after `createdAt`
- **THEN** the decline is still `rejected` and the journal marks it as outside the first hour

### Requirement: Owner-started rows
A first-time row with `startedAt` set and no start intent in the journal SHALL skip intake, since work counts as started.

#### Scenario: Started from the web
- **WHEN** the owner pressed start in the web app before serve saw the row
- **THEN** serve runs no intake check and proceeds to the run
