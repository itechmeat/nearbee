# hire-serving Specification

## Purpose
Defines how `nearbee serve` takes first-time hires for a published listing, runs them through apb and delivers or declines them.

## Requirements

### Requirement: Polling
`serve` SHALL poll every configured agent's assignments in all statuses every 30 to 60 seconds and act only on `in_progress` first-time rows of agents that have a published listing.

#### Scenario: Rework row
- **WHEN** a row is rework, a dispute redo, or submitted with a new message
- **THEN** serve logs it and takes no action on it

#### Scenario: Cancelled row
- **WHEN** a row carries a `cancelReason` or a terminal status while local work runs for it
- **THEN** serve stops waiting on that hire and records it

#### Scenario: Revoked token
- **WHEN** polling an agent answers unauthorized
- **THEN** serve stops polling that agent, tells the operator, and keeps serving the others

### Requirement: Taking a hire
For a new first-time row, serve SHALL record `received`, start the assignment, start one apb run of the listing's materialised playbook in a fresh worktree with a fresh `execution_id`, and wait for it with a wall-clock limit ending 10 minutes before the row's `slaDeadlineAt`. The buyer's brief reaches the playbook only as the untrusted `brief` run parameter (rule 3); the run instruction is fixed adapter-owned text, `hire_id` and `execution_id` travel in the start request.

#### Scenario: Brief is a parameter, never the instruction
- **WHEN** serve starts the run for a hire with a buyer brief
- **THEN** the start body carries the brief only in the untrusted `brief` param and the instruction is fixed text

#### Scenario: Owner already started it
- **WHEN** the row has `startedAt` set and the journal has no start intent
- **THEN** serve does not call start again and proceeds to the run

### Requirement: Safe declines and submits
Decline reasons sent to the buyer SHALL be short and generic (no paths, no apb internals); details go to the journal and the operator. Before any submit, serve SHALL refuse a deliverable containing an `aat_` token, a configured secret or a stored agent token, declining `failed` with a generic reason.

#### Scenario: A token in the deliverable is refused
- **WHEN** the run's deliverable Markdown contains a stored or configured secret
- **THEN** serve submits nothing and declines with `failed` and a generic reason

### Requirement: No duplicate run or submit
Serve SHALL write an intent before starting the assignment, before starting the run and before submitting, and after a restart SHALL resume from the journal: a run intent without a recorded run id SHALL be resolved by looking the run up by `execution_id` before any new start; a submit intent SHALL be completed by resending the identical submission.

#### Scenario: Restart after the run started
- **WHEN** serve restarts with a run intent and the lookup finds the run
- **THEN** it follows that run and starts no other

#### Scenario: Restart, lookup incomplete
- **WHEN** the lookup finds nothing and reports itself incomplete
- **THEN** serve starts no run and reports the hire for the operator

### Requirement: Delivery
On a succeeded run serve SHALL validate the manifest and submit its Markdown; on a failed or timed-out run, or an invalid manifest, it SHALL decline with `failed` and a short reason; a run report or summary SHALL never be submitted.

#### Scenario: Invalid manifest
- **WHEN** the run succeeded but its manifest fails validation
- **THEN** serve declines with `failed` and submits nothing

### Requirement: Refused or unknown submit
A submit the market refuses SHALL decline with `failed` naming the machine code when the row is still `in_progress`, and otherwise SHALL be recorded and left. A submit whose outcome stays unknown after the client's one resend SHALL be recorded and reported to the operator, its intent SHALL stay open for the next restart, and no second deliverable SHALL be built.

#### Scenario: Submit refused
- **WHEN** submit answers `400 invalid_deliverable` and the row is still `in_progress`
- **THEN** serve declines with `failed` naming `invalid_deliverable`

#### Scenario: Submit outcome unknown
- **WHEN** submit and its one resend both end with an unknown outcome
- **THEN** serve records it, reports the hire to the operator, keeps the submit intent open and builds no second deliverable

### Requirement: Test decline
For the x402 proof, serve SHALL support declining new first-time hires of one named agent with `rejected` before any work (`serve --reject <handle>`).

#### Scenario: Reject mode
- **WHEN** serve runs with `--reject repo-audit-fast` and a first-time hire arrives for it
- **THEN** it declines with `rejected` and starts nothing

### Requirement: Served version
Serve SHALL run only the playbook version recorded for the listing at publish time, record that version and digest in each hire's run intent, start a new run only when apb names that version as current, and stop a run that reports another version or none.

#### Scenario: apb runs another version
- **WHEN** a new hire arrives and apb names a current version other than the recorded one
- **THEN** serve starts no run and declines the hire with `failed`, naming both versions in the journal

#### Scenario: A run reports another version
- **WHEN** a run's detail reports a version other than the recorded one, or no version
- **THEN** serve stops the run, never submits its output, and declines the hire with `failed`

#### Scenario: apb names no current version
- **WHEN** a new hire arrives and apb's playbook list names no current version for the listing's playbook
- **THEN** serve starts nothing and reports the hire to the operator

### Requirement: Version snapshot per hire
Serve SHALL read the resolved listing records again at the start of every polling round, so a new hire takes the listing's current version. A hire whose journal holds a run intent, open or closed, SHALL be checked against the version and digest that intent recorded, also after a restart.

#### Scenario: Restart after a republish
- **WHEN** a hire's run intent recorded version v1, the listing is then republished as v2 and serve restarts
- **THEN** every check of that hire uses v1: a v1 run is followed and delivered, and an unresolved v1 run intent never starts a v2 run

#### Scenario: Republish while serve runs
- **WHEN** the listing is republished as v2 while serve runs and a new hire arrives afterwards
- **THEN** serve reads the v2 record on its next round, without a restart, and serves the new hire on v2
