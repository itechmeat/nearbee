# Spec Delta

## Purpose
Defines `nearbee replay`, the credential-free walkthrough that runs the real hire state machine against scripted market, runner and attestor implementations so a judge sees a hire end to end on a clean checkout.

## ADDED Requirements

### Requirement: Scenarios
`nearbee replay` SHALL run the scenarios `success`, `decline`, `redo`, `restart` and `cancellation` in that order through the real `serve::round`, or only those named by repeated `--scenario <name>` options, and SHALL check for each the hire's end state in its journal, the ordered market writes, the number of run starts and, for `success` and `redo`, the receipt link in the submitted deliverable.

#### Scenario: Success
- **WHEN** the `success` scenario runs
- **THEN** the hire is started once, one run starts, the deliverable is submitted with a Verification Center receipt link, the buyer accepts and the scenario prints `ok`

#### Scenario: Decline at intake
- **WHEN** the `decline` scenario sends a brief that names no GitHub repository
- **THEN** the hire is declined `rejected` by the intake check with no market start and no run start

#### Scenario: Redo
- **WHEN** the buyer asks for a rework after the first submit
- **THEN** a second run starts with `continued_from` the first run and the feedback param, and the hire is submitted again

#### Scenario: Restart
- **WHEN** the first run start's answer is lost (start outcome unknown) and the in-memory serve state is dropped and rebuilt over the same state directory
- **THEN** the open start intent is resolved by finding the run by `hire_id`, no second start is sent, and the hire is delivered

#### Scenario: Cancellation
- **WHEN** the buyer cancels while the run is running
- **THEN** the run is aborted, the hire is closed and nothing is submitted

### Requirement: Isolation
The replay SHALL read no configuration file, credential store, `.env` or `NEARBEE_` variable other than the log filter `NEARBEE_LOG`, SHALL open no network connection and SHALL start no process; its fixtures SHALL be embedded in the binary.

#### Scenario: Clean machine
- **WHEN** the replay runs with an empty home directory and no apb installed
- **THEN** every scenario passes

### Requirement: Output and exit codes
The replay SHALL print one line per step with the hire state and the market write and no identifier, then one line per scenario with `ok` or `failed` (a failure names the expected and the actual value), and SHALL exit 0 when every selected scenario passes, 1 when one fails and 2 on an unknown scenario name. Its state SHALL live in a fresh temporary directory removed at exit, or in the empty or new directory `--keep <dir>` names, which is kept.

#### Scenario: Unknown scenario
- **WHEN** `nearbee replay --scenario nope` runs
- **THEN** it prints the known names and exits 2 without running anything

#### Scenario: Verbose
- **WHEN** `--verbose` is given
- **THEN** each step line also names the journal events written in it

#### Scenario: Kept state
- **WHEN** `--keep` names a directory that holds files
- **THEN** the replay refuses before running and exits 2

### Requirement: Honest narration
The replay SHALL state at its start that market rows are shaped like the recorded market answers, that the transitions are scripted, that no market, apb or NEAR AI Cloud is contacted, and that the response signature and its receipt link are synthetic and will not verify.

#### Scenario: Banner
- **WHEN** the replay starts
- **THEN** its first lines say that it is a scripted walkthrough that contacts nothing
