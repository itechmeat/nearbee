# Spec Delta

## MODIFIED Requirements

### Requirement: Run start
The runner SHALL start a run of a playbook with an instruction, string params, an existing worktree, an optional budget (when present it has a dollar limit and an explicit `on_no_estimate`; when absent the request carries no budget object) and an optional `continued_from` run id, and SHALL always add the params `hire_id` and `execution_id` from the caller. It SHALL never retry a start on its own, at any layer: the HTTP client sends each start request at most once (no transport-level resend, also on an HTTP/2 refused stream or `GOAWAY`).

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

## ADDED Requirements

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
