# Spec Delta

## ADDED Requirements

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
