# hire-serving Specification

## Purpose
Defines how `nearbee serve` takes first-time hires for a published listing, runs them through apb and delivers or declines them.

## Requirements

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
For a new first-time row, serve SHALL record `received`, run intake (spec `hire-intake`), start the assignment, download the attachments (spec `hire-attachments`), and start one apb run of the listing's materialised playbook in the hire's worktree with a fresh `execution_id`. The buyer's title and description reach the playbook only as the untrusted `brief` param, the attachments only as the `inputs` param (rule 3); the run instruction is fixed adapter-owned text.

#### Scenario: Brief is a parameter, never the instruction
- **WHEN** serve starts the run for a hire with a buyer brief
- **THEN** the start body carries the title and description only in the untrusted `brief` param and the instruction is fixed text

#### Scenario: Owner already started it
- **WHEN** the row has `startedAt` set and the journal has no start intent
- **THEN** serve skips intake and the start call and proceeds to the attachments and the run

#### Scenario: Declined at intake
- **WHEN** intake declines a hire
- **THEN** serve never calls start and never starts a run for it

### Requirement: Safe declines and submits
Decline reasons sent to the buyer SHALL be short and generic (no paths, no apb internals); details go to the journal and the operator. Before any submit, serve SHALL refuse a deliverable containing an `aat_` token, a configured secret or a stored agent token, declining `failed` with a generic reason.

#### Scenario: A token in the deliverable is refused
- **WHEN** the run's deliverable Markdown contains a stored or configured secret
- **THEN** serve submits nothing and declines with `failed` and a generic reason

### Requirement: No duplicate run or submit
Serve SHALL write an intent before every external write (market start, decline, submit, progress, apb start and abort) and after a restart SHALL resume from the journal. An unresolved apb start SHALL be resolved only by an `execution_id` lookup that finds exactly one run; serve SHALL never start a second run for a generation whose start is unresolved. An open submit intent SHALL be settled against the row first and resent from the generation's snapshot only while the row still awaits it.

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
On a succeeded run serve SHALL validate the manifest, recheck any `tee_signature` evidence, append the signature, evidence (attested hires) and statement sections, check the Markdown limit and the secret scan, deliver the output files, snapshot the result and submit it; a failed, aborted or timed-out run or an invalid manifest SHALL be declined `failed` with a short reason; a run report SHALL never be submitted.

#### Scenario: Invalid manifest
- **WHEN** the run succeeded but its manifest fails validation
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Footer pushes the deliverable over the limit
- **WHEN** the playbook's Markdown fits the limit but with the statement appended it does not
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Manifest with output files
- **WHEN** a succeeded run's manifest lists `flow.json` beside the Markdown
- **THEN** serve uploads `flow.json`, checks the size and digest the complete reports against the manifest (when the complete answers `409 upload_not_pending` and reports none, the check is the successful PUT of the exact bytes to the upload URL signed for the reserved size), and submits the Markdown with that file's reference

#### Scenario: Rate-limited upload
- **WHEN** the market answers an upload write with `429`
- **THEN** serve leaves the delivery pending, counts one of the file's three attempts, and uploads the file again on a later round instead of declining

#### Scenario: Crash after the reserve
- **WHEN** serve restarts after a file's reserve was journaled but before its complete was
- **THEN** it records the upload as lost, deletes the reserved file, and uploads the file again

#### Scenario: Crash after a deletion
- **WHEN** serve restarts after it deleted uploaded files and before it recorded the decline
- **THEN** no deleted file is reused: the journal records each deletion before the call, and a later submit carries only files uploaded again

#### Scenario: Resume after a crash
- **WHEN** serve restarts after a file upload completed but before the submit
- **THEN** it submits with the recorded file reference and does not upload the file again

#### Scenario: Too many files
- **WHEN** a manifest lists 21 files
- **THEN** serve declines `failed` naming the 20-file limit and uploads nothing

#### Scenario: Recovered submit
- **WHEN** serve restarts after the submission snapshot was written
- **THEN** it resends the snapshotted bytes, statement, evidence and file references included, without assembling them again

### Requirement: Output files
Serve SHALL upload every output file the manifest lists through the market's file API within the file limits and SHALL submit the Markdown with the file references; a file past a limit or with a secret SHALL decline the hire `failed` with the reason.

#### Scenario: Secret in an output file
- **WHEN** a listed output file contains a token the secret scan finds
- **THEN** serve declines `failed` and submits nothing

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

### Requirement: Version snapshot per hire
Serve SHALL read the resolved listing records again at the start of every polling round, so a new hire takes the listing's current version. A hire whose journal holds a run intent, open or closed, SHALL be checked against the version and digest that intent recorded, also after a restart.

#### Scenario: Restart after a republish
- **WHEN** a hire's run intent recorded version v1, the listing is then republished as v2 and serve restarts
- **THEN** every check of that hire uses v1: a v1 run is followed and delivered, and an unresolved v1 run intent never starts a v2 run

#### Scenario: Republish while serve runs
- **WHEN** the listing is republished as v2 while serve runs and a new hire arrives afterwards
- **THEN** serve reads the v2 record on its next round, without a restart, and serves the new hire on v2

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

### Requirement: Drifted listing
Once per round, before intake, serve SHALL check every listing for drift (spec `data-statement`, Drift). A hire of a drifted listing whose generation has no `run_start` intent SHALL get no scope check, market start or apb start and SHALL be declined at once, `rejected` when first-time and not started on the market, else `failed`, with attention `listing_drift`; reconciliation, aborts, delivery and pending writes of started hires SHALL continue.

#### Scenario: Profile edited while serving
- **WHEN** the `nearbee-near` profile gains an incognito fallback while serve runs
- **THEN** the next waiting hire of the attested listing is declined and no run starts

#### Scenario: Decisions turned on before intake
- **WHEN** the project's decisions are enabled before a new hire of the attested listing is taken
- **THEN** the hire is declined `rejected` and no scope check is asked

#### Scenario: Started hire still delivers
- **WHEN** a hire's run is already running when its listing drifts
- **THEN** serve still observes the run and delivers it with the statement snapshotted at its start

#### Scenario: Repeat drift
- **WHEN** a listing drifts, is clean again in a later round, and drifts once more
- **THEN** the operator is told about each drift once

### Requirement: Held listing
When the inspection of a listing fails in a round, serve SHALL hold that listing for the round: a hire whose generation has no `run_start` intent SHALL get no intake check, scope check, market start or apb start and SHALL not be declined; the waiting-past-deadline rule SHALL decline it in the end. Only a fingerprint, statement or tier-check mismatch on facts that were read is drift that declines at once. A listing record that is not serveable SHALL never start a generation.

#### Scenario: apb restarting
- **WHEN** the profile detail route times out in one round
- **THEN** no hire of that listing starts or is declined in that round, and the next round checks again

#### Scenario: Held past the deadline
- **WHEN** a hire's listing stays held until the hire's deadline passes
- **THEN** the hire is declined by the waiting rule, not as drift

### Requirement: Generation statement snapshot
Before the market start and the apb start of every generation serve SHALL write the listing's tier, statement text and hash, statement format, fingerprint and evidence model set into the generation's snapshot and name the hash and fingerprint in the `run_start` intent; delivery and recovery SHALL use that snapshot, never the current record. A snapshot left without an intent SHALL be reused only when it matches the listing.

#### Scenario: Snapshot first
- **WHEN** a run start intent is journaled
- **THEN** the generation's statement snapshot is already on disk

#### Scenario: Republished before the intent
- **WHEN** serve stopped after the snapshot, and the listing was listed again with other facts
- **THEN** the old snapshot is not reused and the hire is declined with `listing_drift`

### Requirement: Legacy generations
A generation whose `run_start` intent has no `statement_sha256` reference (journaled before this change) SHALL deliver without footer or evidence; a generation whose intent has it SHALL fail closed (`failed`, attention `statement_missing`) when its snapshot is missing or does not match the hash.

#### Scenario: S3 hire
- **WHEN** an S3 hire's run succeeds after the upgrade
- **THEN** it is delivered as in S3

#### Scenario: Damaged S4 snapshot
- **WHEN** an S4 generation's `statement.json` is missing at delivery
- **THEN** the hire is declined `failed` and nothing is submitted

### Requirement: Response signature recheck
For a `tee_signature` entry serve SHALL pair it with exactly one `tee_request` and one `tee_response` entry in its directory, recompute both SHA-256 from the validated bytes, rebuild `<model>:<request hash>:<response hash>`, compare it with the entry, require `signature_kind` `provider_tee` and a frozen direct model, and snapshot the two private files; any mismatch SHALL decline `failed` with attention `signature_mismatch`.

#### Scenario: Response file edited
- **WHEN** `response.bin` changed after the signature was fetched
- **THEN** serve declines `failed` with `signature_mismatch` and submits nothing

#### Scenario: Gateway signature
- **WHEN** the entry's `signature_kind` is `gateway`
- **THEN** serve declines `failed` with `signature_mismatch`

### Requirement: Signature field formats
Before rendering, serve SHALL require `completion_id` to match `[A-Za-z0-9_-]{1,128}`, `signing_address` `0x` plus 40 hex digits, `signature` `0x` plus 130 hex digits, both hashes 64 lowercase hex digits, `signing_algo` `ecdsa`, and `model` in the frozen direct models; anything else SHALL decline `failed` with attention `signature_mismatch`.

#### Scenario: Link in the completion id
- **WHEN** `completion_id` holds a Markdown link
- **THEN** serve declines `failed` with `signature_mismatch`

### Requirement: Required signature
For a generation whose frozen snapshot has `signed_models`, serve SHALL require one valid `tee_signature` entry whose model is in that list, and SHALL decline `failed` with attention `signature_missing` when there is none. A statement snapshot that cannot be read or parsed SHALL never read as no direct or signed models: at the run start and at delivery it SHALL decline `failed` with attention `statement_missing`; only a legacy generation without a snapshot reads as none.

#### Scenario: Unreadable snapshot at delivery
- **WHEN** a statement generation's snapshot does not parse when its run has ended
- **THEN** serve declines `failed` with `statement_missing`, submits nothing, and never skips the signature requirement

#### Scenario: Signed listing without the entry
- **WHEN** a `doc-digest-private` run succeeds with a manifest that has no `tee_signature` entry
- **THEN** serve declines `failed` with `signature_missing` and submits nothing

### Requirement: Response signature section
The deliverable SHALL carry a section `Response signature`, between the playbook Markdown and the evidence section, with the completion id, model, signing address, both hashes, the signed text and the signature each in a code span, and the link `https://cloud.near.ai/verify?model=<URL-encoded id>#receipt=<receipt>` as a bare URL alone on its line; the private bytes SHALL never be delivered.

#### Scenario: Receipt link
- **WHEN** an agent B deliverable is assembled
- **THEN** its receipt is compact JSON with the keys `completionId`, `model`, `signatureKind`, `signingAlgo`, `signingAddress`, `signedText` and `signature` in that order and the entry's values, encoded base64url without padding

#### Scenario: Link alone on its line
- **WHEN** an agent B deliverable is assembled
- **THEN** the receipt link is the whole of its line and never inside Markdown link syntax, because a buyer reading the raw text would take the closing parenthesis with it and the Verification Center rejects that receipt as invalid

### Requirement: What the buyer can verify
The section SHALL say that the signature binds a response with the shown hash produced in a model TEE, and that the buyer cannot recompute the hashes because the request and response bytes stay with the owner.

#### Scenario: Wording
- **WHEN** an agent B deliverable is assembled
- **THEN** the section states that the request and response bytes stay with the owner and the hashes cannot be recomputed by the buyer

### Requirement: apb version gate at intake
At intake of a new first-time hire, before the market start, serve SHALL ask the runner whether external-data runs are supported; when they are not it SHALL decline the hire `rejected` with "the agent cannot take hires right now" and raise `apb_too_old`, and on an error it SHALL leave the row for the next round.

#### Scenario: Old apb at intake
- **WHEN** external-data runs are not supported and a new hire arrives
- **THEN** the hire is declined `rejected` before any market start and `apb_too_old` is raised

#### Scenario: Unreadable version
- **WHEN** the runner answers the support question with a protocol error at intake
- **THEN** nothing is written for the row and serve retries next round

#### Scenario: Supported
- **WHEN** external-data runs are supported
- **THEN** the market start and the run start proceed

### Requirement: apb version gate before a run
Before every run start of a hire (first run, rework, redo) serve SHALL ask again and start nothing when external-data runs are not supported or the answer is an error; the hire then keeps its state with `apb_too_old`, or, on an error, an operator message shown once per hire and process (the notice key `apb_version:<row id>` the intake gate uses) while later rounds ask again silently.

#### Scenario: apb downgraded before a rework
- **WHEN** external-data runs are not supported when a rework run would start
- **THEN** no run start is sent and the hire shows `apb_too_old`

#### Scenario: Unreadable version before a rework
- **WHEN** the support question fails on several rounds before a rework run would start
- **THEN** no run start is sent, the operator is told once, and the run starts once the answer is yes

### Requirement: Every market run is an external-data run
Every run serve starts for a hire SHALL be started with the external-data flag, and its run start intent SHALL record `external_data: true`.

#### Scenario: Flag on the start
- **WHEN** serve starts the first run of a hire
- **THEN** the start request carries the external-data flag and the run start intent records `external_data: true`

### Requirement: External-data flag check
On every observed run detail of a run whose start intent recorded the flag (the first observation, reworks and redos included) serve SHALL check that the run reports the flag. When it does not, serve SHALL abort the run, journal `privacy_flag_missing` with the run id, decline the hire `failed` with the reason "the run could not be isolated", and raise the operator attention `privacy_incident`.

#### Scenario: Flag missing on the first run
- **WHEN** the first observed detail of a hire's first run does not report the flag
- **THEN** serve aborts the run, journals `privacy_flag_missing`, declines the hire `failed` and raises `privacy_incident`

#### Scenario: Flag missing on a continued run
- **WHEN** a rework run's detail does not report the flag
- **THEN** serve aborts that run, declines the hire `failed` with "the run could not be isolated" and raises `privacy_incident`

### Requirement: Runs started before the flag
A run whose start intent has no `external_data` field (started before the flag was recorded) SHALL not be checked, so an upgrade does not stop hires in flight.

#### Scenario: Run started before the flag
- **WHEN** a run whose start intent has no `external_data` field reports no flag
- **THEN** serve does not abort it and the hire proceeds

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
