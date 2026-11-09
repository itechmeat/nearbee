# Spec Delta

## Purpose

Defines what Nearbee requires from a local apb dashboard and how it starts, observes and finds playbook runs through it.

## ADDED Requirements

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
The runner SHALL start a run of a playbook with an instruction, string params, an existing worktree and an explicit budget policy, and SHALL always add the params `hire_id` and `execution_id` from the caller. It SHALL never retry a start on its own, at any layer: the HTTP client sends each start request at most once (no transport-level resend, also on an HTTP/2 refused stream or `GOAWAY`).

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
The runner SHALL read a run's status, per-node status and outputs, worktree and usage, map `succeeded`, `failed` and `aborted` to terminal states, treat `created`, `running`, `paused` and `interrupted` as not terminal, and treat any unknown status as not terminal and not successful.

#### Scenario: Unknown status
- **WHEN** the dashboard reports a status the runner does not know
- **THEN** the run is reported as unknown, neither terminal success nor failure

#### Scenario: Missing cost
- **WHEN** usage has no cost
- **THEN** the cost is reported as unknown, not zero

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

