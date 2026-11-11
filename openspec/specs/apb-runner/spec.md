# apb-runner Specification

## Purpose
Defines what Nearbee requires from a local apb dashboard and how it starts, observes and finds playbook runs through it.

## Requirements

### Requirement: Workspace pinning
The runner SHALL resolve the configured apb project path to its workspace id through the dashboard's project list once at start-up and SHALL pass that workspace on every call that accepts one (playbook detail, estimate, run start, run detail, run search).

#### Scenario: Workspace resolved
- **WHEN** the configured path matches one apb project
- **THEN** every later request carries that project's workspace id

#### Scenario: Playbook from another project
- **WHEN** the playbook list contains a playbook of another workspace with the same id
- **THEN** the runner only considers the one of the pinned workspace

### Requirement: Authentication
The runner SHALL send `Authorization: Bearer <key>` when an apb API key is configured and no credential otherwise, SHALL not follow redirects, and SHALL not use system proxies.

#### Scenario: Key configured
- **WHEN** `APB_API_KEY` is set
- **THEN** every request carries the bearer header and the key never appears in logs or errors

### Requirement: Run start
The runner SHALL start a run of a playbook with an instruction, string params, an existing worktree, an optional budget (when present it has a dollar limit and an explicit `on_no_estimate`; when absent the request carries no budget object) and an optional `continued_from` run id, and SHALL always add the params `hire_id` and `execution_id` from the caller. It SHALL never retry a start on its own, at any layer: each start request is sent at most once, with no transport-level resend.

#### Scenario: Successful start
- **WHEN** the dashboard accepts the start
- **THEN** the runner returns the run id and the request carried `hire_id` and `execution_id`

#### Scenario: Caller passes a reserved param
- **WHEN** the caller's params contain `hire_id` or `execution_id`
- **THEN** the runner refuses with an invalid-request error before sending anything

#### Scenario: Queued start
- **WHEN** the dashboard answers 200 with a run id while the workdir is busy
- **THEN** the start is known and successful; the run is observed like any other

#### Scenario: Unusable run id on a success
- **WHEN** the dashboard answers 200 with a `run_id` that is empty or not one URL path segment
- **THEN** the result is start-outcome-unknown, not a known start

#### Scenario: Refused HTTP/2 stream
- **WHEN** the connection to the dashboard is HTTP/2 and it refuses the start's stream
- **THEN** the request is not sent again and the result is start-outcome-unknown

#### Scenario: Missing worktree
- **WHEN** the worktree path does not exist
- **THEN** the runner refuses before sending the request

#### Scenario: Budget refusal
- **WHEN** the dashboard answers 409 `budget_needs_decision`
- **THEN** the runner returns a typed refusal with the code and the estimate, and does not adjust the budget

#### Scenario: No budget
- **WHEN** the caller passes no budget
- **THEN** the start body has no `budget` key

#### Scenario: Continued run
- **WHEN** the caller passes `continued_from: run-1`
- **THEN** the start body carries `continued_from: "run-1"`

### Requirement: Error classes
Every failure of a call that reached the dashboard, other than a 404 on a read, SHALL be one of: refused (a known pre-start gate, with HTTP status and machine code), retryable read (transport error or a retryable server status on a read), start outcome unknown (timeout, connection reset, server error, redirect or malformed success on a start), protocol (an answer the runner cannot understand), each with a bounded, redacted detail.

#### Scenario: Launch gate
- **WHEN** the dashboard refuses a start because the playbook needs launch confirmation
- **THEN** the result is refused with the code, and the runner never sends a confirmation itself

#### Scenario: Timeout on start
- **WHEN** the start request times out after being sent
- **THEN** the result is start-outcome-unknown, not refused and not retried

#### Scenario: Plain-text error body
- **WHEN** the dashboard answers 422 with a plain-text body
- **THEN** the result is refused with status 422 and the text as detail

### Requirement: Not found and invalid request
A read answered 404 SHALL fail as not found, distinct from protocol and retryable read, and SHALL not be retried. A caller error caught before anything is sent (a reserved param, a missing worktree, an id that is not one URL path segment) SHALL fail as invalid request. The not-found detail is bounded and redacted like every other error detail.

#### Scenario: Unknown run on a read
- **WHEN** the dashboard answers 404 to a read of a run
- **THEN** the result is not found and the read was sent once

#### Scenario: Id that is not one path segment
- **WHEN** a run or playbook id is empty, `.`, `..` or contains `/`, `%` or a space
- **THEN** the result is invalid request and nothing is sent

### Requirement: Run observation
The runner SHALL read a run's status, per-node status and outputs, worktree, version, usage (total `cost_usd`, the split by basis, and the counters `attempts`, `finished_attempts`, `cost_attempts`) and budget pause, map `succeeded`, `failed` and `aborted` to terminal states, treat `created`, `running`, `paused` and `interrupted` as not terminal, and treat any unknown status as not terminal and not successful.

#### Scenario: Unknown status
- **WHEN** the dashboard reports a status the runner does not know
- **THEN** the run is reported as unknown, neither terminal success nor failure

#### Scenario: Missing cost
- **WHEN** usage has no cost
- **THEN** the cost is reported as unknown, not zero

#### Scenario: Partial cost
- **WHEN** a terminal run reports `cost_usd` with `cost_attempts` 1 and `finished_attempts` 3
- **THEN** the runner reports the cost as incomplete

#### Scenario: Budget pause
- **WHEN** the run detail reports `budget.paused`
- **THEN** the runner reports the run as paused by its budget

### Requirement: Run lookup
The runner SHALL find runs by a search field (`hire_id` or `execution_id`) within the pinned workspace and playbook and SHALL return the matching runs together with whether the search was complete (no runs withheld or not indexed).

#### Scenario: Incomplete search
- **WHEN** the dashboard reports withheld or not-indexed runs
- **THEN** the lookup result says it is incomplete even if it has matches

### Requirement: Run find command
`nearbee runs find` SHALL take exactly one of `--hire-id` or `--execution-id`, print one line per matching run (run id, playbook, status) followed by a line stating whether the search was complete, and exit 0 when it found runs, 1 when it found none and the search was complete, and 2 when it found none and the search was incomplete.

#### Scenario: Match found
- **WHEN** one run carries the given `hire_id`
- **THEN** the command prints that run and `complete: yes` and exits 0

#### Scenario: No match, complete
- **WHEN** no run matches and nothing was withheld or left unindexed
- **THEN** the command prints `complete: yes` and exits 1

#### Scenario: No match, incomplete
- **WHEN** no run matches but the dashboard reports unindexed runs
- **THEN** the command prints `complete: no` and exits 2, since absence is not proven

### Requirement: Abort
The runner SHALL abort a run with `apb stop <run_id>` against Nearbee's own instance (its `APB_CONFIG_DIR`, run from the workspace), never through a drain pause, and SHALL report success only when the command exits 0; the caller confirms the terminal status by observation.

#### Scenario: Abort of a finished run
- **WHEN** the run is already terminal
- **THEN** the abort reports success or a known "not running" answer, and the caller observes the terminal status

### Requirement: Decide
The runner SHALL ask a yes or no question with `apb decide is <question> --criteria <text> --no-run` against Nearbee's own instance, passing each argument directly (no shell), with `APB_RUN_ID` and `APB_NODE_ID` removed from the child's environment and a 60-second timeout, and SHALL return answered yes, answered no, or unanswered with apb's refusal code or the error; a child killed at its limit is a runner error, which the caller maps.

#### Scenario: No provider
- **WHEN** apb prints `{"answered":false,"refused":"no_provider"}`
- **THEN** the runner returns unanswered with `no_provider`

#### Scenario: Timeout
- **WHEN** the command runs longer than 60 seconds
- **THEN** the child is killed and the runner returns an error naming the limit (`did not finish within`); serve reads that error as unanswered with `timeout` (or `cut_off` when the caller's limit was shorter), so the scope check is skipped

### Requirement: Requested models per attempt
Run observation SHALL keep apb's per-attempt model records: node id, attempt number, agent, the requested model when recorded, and the reported identity as none, one model, or mixed. A missing reported model SHALL stay empty, never filled from the requested one. The terminal observation journaled for a run SHALL carry these records, so delivery after a restart needs no apb read.

#### Scenario: No reported model
- **WHEN** an attempt record's `reported` is `{"kind": "none"}`
- **THEN** the observed attempt has the requested model and no reported model

#### Scenario: Restart after terminal observation
- **WHEN** serve restarts after journaling a succeeded run and apb is unreachable
- **THEN** the disclosure is built from the journaled attempt records

### Requirement: Run parameters for a script node
`nearbee-apb` SHALL provide the run's string params to a Nearbee command started by a script node, read from the run directory apb names in `APB_RUN_DIR`, so no other crate knows apb's run file layout.

#### Scenario: Brief param
- **WHEN** a script node runs `nearbee audit fetch` with `APB_RUN_DIR` set
- **THEN** the command receives the run's `brief` param as a string

### Requirement: Playbook inspection
The runner SHALL report the execution facts of an installed playbook (spec `privacy-tiers`, Execution facts) from apb's playbook and profile detail routes, the apb unit's environment and the workspace's provider and project configuration files. It SHALL report each fact the configuration does not give (an absent key, a 404, a file that does not parse) as unknown with the reason.

#### Scenario: Profile route error
- **WHEN** the profile detail route answers 404 for a profile the playbook names
- **THEN** the facts carry that profile as unknown with the HTTP status as the reason

### Requirement: Failed inspection reads
The inspection SHALL fail as a whole on a failed read of the instance (a transport error, a server error, a `systemctl` that fails), so a transient error never changes the facts or their fingerprint.

#### Scenario: Transient error
- **WHEN** the profile detail route answers 503 or the health route fails
- **THEN** the inspection fails and no fact is reported

### Requirement: Instance version
The runner SHALL read the instance version from `GET /api/health`, parse it as a semantic version with prerelease order, and report external-data runs as supported when the version is at least `1.0.0-beta.7`; a missing or unparsable version SHALL be a protocol error.

#### Scenario: Prerelease order
- **WHEN** the instance reports `1.0.0-beta.10`
- **THEN** external-data runs are supported, as they are for `1.0.0`, and not for `1.0.0-beta.6`

#### Scenario: Unparsable version
- **WHEN** the health answer's version is `dev`
- **THEN** the runner returns a protocol error naming the field

### Requirement: External-data runs
A start request SHALL carry `external_data: true` in the body when the caller sets the flag and no `external_data` key when it does not, and run observation SHALL report the run as an external-data run only when the run detail carries `external_data: true`.

#### Scenario: Flag sent
- **WHEN** the caller starts a run with the flag set
- **THEN** the start body carries `"external_data": true`

#### Scenario: Flag not set
- **WHEN** the caller starts a run without the flag
- **THEN** the start body has no `external_data` key

#### Scenario: Flag absent in the detail
- **WHEN** a run detail has no `external_data` field
- **THEN** the run is reported as not an external-data run
