# Spec Delta

## MODIFIED Requirements

### Requirement: Polling
`serve` SHALL poll every configured agent's assignments in all statuses every 30 to 60 seconds, counted from the start of the previous round, and in each round SHALL first reconcile every row whose hire has a journal, then act on new `in_progress` first-time rows of agents that have an installed listing.

#### Scenario: Rework row
- **WHEN** a journaled hire's row is `in_progress` with `deliverableUrl` and `submittedAt` set
- **THEN** serve opens the hire's next generation as rework (requirement Rework and redo)

#### Scenario: Cancelled row
- **WHEN** a row carries a `cancelReason` or a terminal status while local work runs for it
- **THEN** serve aborts the hire's run, records the row's status and reason, and closes the hire

#### Scenario: Revoked token
- **WHEN** polling an agent answers unauthorized
- **THEN** serve stops polling that agent, tells the operator, and keeps serving the others

#### Scenario: Row without a journal in another status
- **WHEN** a row that has no journal is not `in_progress`
- **THEN** serve leaves it and writes nothing

### Requirement: Taking a hire
For a new first-time row, serve SHALL record `received`, run intake (spec `hire-intake`), start the assignment, download the attachments (spec `hire-attachments`), and start one apb run of the listing's materialised playbook in the hire's worktree with a fresh `execution_id`. The buyer's title and description reach the playbook only as the untrusted `brief` param, the attachments only as the `inputs` param (rule 3); the run instruction is fixed adapter-owned text, `hire_id` and `execution_id` travel in the start request.

#### Scenario: Brief is a parameter, never the instruction
- **WHEN** serve starts the run for a hire with a buyer brief
- **THEN** the start body carries the title and description only in the untrusted `brief` param and the instruction is fixed text

#### Scenario: Owner already started it
- **WHEN** the row has `startedAt` set and the journal has no start intent
- **THEN** serve skips intake and the start call and proceeds to the attachments and the run

#### Scenario: Declined at intake
- **WHEN** intake declines a hire
- **THEN** serve never calls start and never starts a run for it

### Requirement: No duplicate run or submit
Serve SHALL write an intent before every external write (market start, decline, submit, progress; apb start and abort) and after a restart SHALL resume from the journal. An apb start intent without a run id SHALL be resolved only by an `execution_id` lookup that finds exactly one run; serve SHALL never start a second run for a generation whose start is unresolved. An open submit intent SHALL be settled against the row first and resent, from the generation's snapshot, only while the row still awaits that submission.

#### Scenario: Restart after the run started
- **WHEN** serve restarts with a run intent and the lookup finds the run
- **THEN** it follows that run and starts no other

#### Scenario: Restart, lookup incomplete
- **WHEN** the lookup finds nothing and reports itself incomplete
- **THEN** serve starts no run and reports the hire for the operator

#### Scenario: Complete empty lookup
- **WHEN** the lookup is complete and empty, every round until the generation's deadline
- **THEN** serve starts no run, sets `start_unresolved` after 120 seconds, declines the generation `failed` at its deadline, and keeps looking in later rounds

#### Scenario: Submit landed, then a dispute redo, before a restart
- **WHEN** a submit's answer was lost, the buyer disputed, the resolver ordered a redo (`submittedAt` null, `deliverableUrl` set, a new `slaDeadlineAt`), and serve restarts with the submit intent open
- **THEN** serve records the submit as landed from the changed generation key, resends nothing, and opens the redo generation

#### Scenario: Submit landed, then rework, before a restart
- **WHEN** a submit's answer was lost, the buyer requested changes, and serve restarts with the submit intent open
- **THEN** serve records the submit as landed from the row's `submittedAt`, resends nothing, and opens the rework generation

#### Scenario: Two runs found
- **WHEN** the `execution_id` lookup returns two runs
- **THEN** serve adopts neither, starts nothing and sets an attention code for the operator

#### Scenario: Submit resend after a restart
- **WHEN** a submit intent is open after a restart and the run's `out/` has changed since
- **THEN** serve resends the Markdown stored in the generation's snapshot, byte for byte, not the changed file

### Requirement: Delivery
On a succeeded run serve SHALL validate the manifest, store the Markdown in the generation's snapshot and submit it; on a failed, aborted or timed-out run, an invalid manifest, or a manifest that lists output files, it SHALL decline with `failed` and a short reason; a run report or summary SHALL never be submitted.

#### Scenario: Invalid manifest
- **WHEN** the run succeeded but its manifest fails validation
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Manifest with output files
- **WHEN** the run's manifest lists files besides the Markdown
- **THEN** serve declines with `failed`, submits nothing, and tells the operator that output files are not supported yet

### Requirement: Served version
Serve SHALL run only the playbook version recorded for the listing at install time, record that version and digest in each generation's run intent, start a new run only when apb names that version as current, and stop a run that reports another version, or that reports no version once it is terminal or has been observed without one for more than 60 seconds.

#### Scenario: apb runs another version
- **WHEN** a new hire arrives and apb names a current version other than the recorded one
- **THEN** serve starts no run and declines the hire with `failed`, naming both versions in the journal

#### Scenario: A run reports another version
- **WHEN** a run's detail reports a version other than the recorded one
- **THEN** serve stops the run, never submits its output, and declines the hire with `failed`

#### Scenario: Version not reported yet
- **WHEN** the first detail of a running run carries no version and the next one, 10 seconds later, names the recorded version
- **THEN** serve follows the run and declines nothing

#### Scenario: Terminal run without a version
- **WHEN** a run is terminal and its detail carries no version
- **THEN** serve never submits its output and declines the hire with `failed`

#### Scenario: apb names no current version
- **WHEN** a new hire arrives and apb's playbook list names no current version for the listing's playbook
- **THEN** serve starts nothing and reports the hire to the operator

## ADDED Requirements

### Requirement: Bounded rounds
No step of a round SHALL wait for a run to end. Each round SHALL reconcile and observe every hire and check every wall clock before any long step (an attachment download or a scope check), SHALL run at most one long step, cut off at the earliest wall clock of any active hire, and a step the market or apb asks to delay (progress too soon, attachment still scanned) SHALL record when it is due and return instead of sleeping.

#### Scenario: An unread hire's wall clock
- **WHEN** a hire's row was not read this round, its recorded wall clock is 20 seconds away, and another hire's scope check or download is due
- **THEN** that long step is cut off at the unread hire's wall clock, and a cut-off scope check leaves no decision process running

#### Scenario: Another hire's cancellation during a long run
- **WHEN** hire A's run is still running and hire B's row turns `cancelled`
- **THEN** B is reconciled in the same round as A's observation, not after A's run ends

#### Scenario: Slow download
- **WHEN** an attachment download of hire A would take 100 seconds and hire B's deadline falls 20 seconds into it
- **THEN** A's download is cut off at B's deadline, B's run is stopped in the next round's short steps at once, and A's download is retried later

#### Scenario: Progress too soon
- **WHEN** a progress write answers `409 progress_report_too_soon` with `retry_after` 12
- **THEN** serve records the due time and sends no progress for that hire before it, without sleeping in the round

### Requirement: One active run
Serve SHALL keep at most one apb run active across all agents: every apb start (first-time, rework, redo) needs the one slot, which an open start intent, a non-terminal run, an open abort, an unreadable journal or a hires directory that cannot be listed completely holds. A waiting first-time hire (intake done, nothing started) whose deadline passes SHALL be declined `rejected`, a waiting rework or redo `failed`.

#### Scenario: The hires directory cannot be listed
- **WHEN** a hire's run is running, the hires directory cannot be listed while each journal can still be read by its path, and another hire is ready to start
- **THEN** nothing starts and the operator is told until the listing works again

#### Scenario: Rework while another hire runs
- **WHEN** a rework row arrives while another hire's run is active
- **THEN** the rework generation is opened and its run starts only after the other run ends

#### Scenario: Second hire while one runs
- **WHEN** a second first-time hire arrives while another hire's run is active
- **THEN** serve runs its intake, does not start it, and starts it in the first round after the active run ends

### Requirement: Lifecycle reconciliation
For a journaled hire, serve SHALL record each new row status once: `accepted` closes the hire and starts its retention clock (from `finalizedAt`, else the first observation); `cancelled` closes it with the `cancelReason`; `disputed` freezes it (no run, no write); `dispute_resolved` closes it without counting as acceptance. Acceptance SHALL never be inferred from `autoAcceptAt`.

#### Scenario: Accepted
- **WHEN** a delivered hire's row reads `accepted` with `finalizedAt`
- **THEN** the journal records `accepted` with that time once, and later rounds write nothing for it

#### Scenario: Disputed
- **WHEN** a delivered hire's row reads `disputed`
- **THEN** serve records `disputed`, starts nothing and sends nothing for it until the row changes

#### Scenario: Silence past autoAcceptAt
- **WHEN** a submitted row's `autoAcceptAt` has passed and the row still reads `submitted`
- **THEN** serve records no acceptance

### Requirement: Generations
A hire SHALL consist of generations: generation 1 is the first-time work; each reopening of the row (rework or dispute redo) opens one next generation, identified by the row's `slaDeadlineAt` together with `deliverableUrl` and `submittedAt`. A generation ends delivered or declined; the hire stays open until a closing row status.

#### Scenario: Repeated polls of one rework row
- **WHEN** the same rework row is seen in three rounds
- **THEN** exactly one generation is opened and exactly one run is started for it

#### Scenario: A second rework
- **WHEN** a delivered rework is reopened again with a new `slaDeadlineAt`
- **THEN** serve opens generation 3

### Requirement: Rework and redo
For a new rework or redo generation serve SHALL start a run with `continued_from` set to the hire's latest run, the original `brief`, the untrusted `feedback` param (requirement Rework feedback), the hire's `inputs` plus the message's attachments, a new `execution_id`, the hire's remaining budget, and an `out/` cleared after the previous one was moved into the previous generation's snapshot. A rework or redo whose remaining budget is not above zero SHALL be declined `failed`.

#### Scenario: Rework
- **WHEN** the buyer requests changes with the message "use a table"
- **THEN** the run's `feedback` param is "use a table", the instruction is the fixed text and `continued_from` names the previous run

#### Scenario: Stale manifest
- **WHEN** the rework run succeeds without writing a new `out/result.json`
- **THEN** serve declines `failed` and does not submit the previous generation's Markdown

#### Scenario: Rework after a republish
- **WHEN** the listing was republished as a new version after the hire's first run and the buyer requests changes
- **THEN** serve starts no run and declines the rework `failed`, naming both versions in the journal

#### Scenario: Budget used up
- **WHEN** a rework arrives and the hire already spent its whole ceiling
- **THEN** serve declines `failed` without starting a run

### Requirement: Rework feedback
The `feedback` param SHALL be the full body of the latest message when its sender side is the buyer (read from the thread when the market truncated it, requirement Thread read of `market-client`), and empty for a redo. When the latest message is not the buyer's, or its full body cannot be read, the rework SHALL be declined `failed` without a run, with the attention code `feedback_unavailable`.

#### Scenario: Redo
- **WHEN** a dispute redo generation starts
- **THEN** `feedback` is empty and no attention is set

#### Scenario: Latest message is not the buyer's
- **WHEN** the latest message on a rework row has the worker sender side
- **THEN** serve starts no run, declines the rework `failed` and sets the attention code `feedback_unavailable`

#### Scenario: Truncated feedback
- **WHEN** the latest buyer message is 6000 characters and the row carries a truncated preview
- **THEN** `feedback` is the full 6000-character body from the thread

### Requirement: Stopping a run
To stop a run serve SHALL write an abort intent, abort the run through the runner, and treat the run as stopped only once its status is terminal; until then the abort SHALL be retried each round and the run SHALL count as active.

#### Scenario: Abort not confirmed
- **WHEN** the abort request times out and the run still reads `running` next round
- **THEN** serve aborts again and starts no other run

### Requirement: Stray runs
Each round serve SHALL look up the runs of every open hire by `hire_id` and abort any run of the hire other than the one adopted for its current generation.

#### Scenario: The execution lookup fails
- **WHEN** a hire's start is unresolved, its lookup by execution id fails, and its lookup by `hire_id` finds a running run under another execution id and one whose params name the unresolved execution
- **THEN** serve aborts the first and leaves the second to the execution lookup

#### Scenario: A late start lands
- **WHEN** a start that was ambiguous lands after serve started the generation again
- **THEN** the second run found by `hire_id` is aborted and its output is never submitted

#### Scenario: Another abort stays unconfirmed
- **WHEN** a hire whose row is not read has a stray whose abort stays unconfirmed, and another stray appears or its unresolved start lands
- **THEN** serve still stops the new stray, or adopts the landed run, in that round

### Requirement: Run budget
Each run SHALL start with `budget.usd` equal to the hire's ceiling (spec `hire-intake`) minus what the hire's earlier runs spent and `on_no_estimate: continue`, or with no budget object when the listing sets `protection: off`. A run that apb pauses for its budget SHALL be stopped and the generation declined `failed`.

#### Scenario: Protection off
- **WHEN** the listing sets `protection: off`
- **THEN** the start request carries no budget object

#### Scenario: Budget pause
- **WHEN** a run reads `paused` with a budget pause
- **THEN** serve aborts it and declines the generation with `failed`

### Requirement: Wall clock
Serve SHALL stop a generation's run at the generation's deadline: the row's `slaDeadlineAt` minus a margin of the smaller of 10 minutes and a fifth of the window from the generation's start, and decline `failed`.

#### Scenario: A ten-minute test run
- **WHEN** a hire's window is 10 minutes
- **THEN** the run may work for 8 minutes before serve stops it

#### Scenario: An open abort of another run
- **WHEN** a hire's row is not read, a stray's abort of that hire stays unconfirmed, and the adopted run reaches the wall clock
- **THEN** serve aborts the adopted run and queues one `failed` decline

#### Scenario: A decline already queued
- **WHEN** a late run was adopted after the generation's decline was queued, and serve stopped before the abort intent
- **THEN** after a restart serve aborts the run, with or without the row, and queues no second decline

#### Scenario: No listing loads
- **WHEN** serve starts with no listing or credentials it can load and a journal on disk has a run active past its recorded wall clock
- **THEN** serve stops the run, confirms the abort, and only then exits with "nothing to serve"

### Requirement: Local work after the market closes
A hire whose row closed (`accepted`, `cancelled`, `dispute_resolved`) or froze (`disputed`) SHALL stay in reconciliation while it has an active run, an open abort or an unresolved start; serve SHALL abort the run and confirm it, and SHALL keep resolving the start, before it treats the hire as settled.

#### Scenario: Accepted while a run is active
- **WHEN** a hire's row turns `accepted` while its rework run is running
- **THEN** serve records `accepted`, aborts the run, and keeps the hire in reconciliation until the run reads terminal

### Requirement: Single serve process
`serve` SHALL hold an exclusive lock on `<state_dir>/serve.lock` for its whole life; a second `serve` or a `cleanup` SHALL exit with code 4 and a message naming the lock while it is held.

#### Scenario: Second serve
- **WHEN** `serve` runs and the operator starts another `serve` on the same state directory
- **THEN** the second exits with code 4 before any market or apb call
