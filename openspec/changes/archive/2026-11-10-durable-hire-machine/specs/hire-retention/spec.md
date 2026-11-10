# Spec Delta

## Purpose

Defines what Nearbee keeps per hire, how the operator sees it (`nearbee hires`) and how it is deleted (`nearbee cleanup`), honestly reporting what Nearbee cannot delete.

## ADDED Requirements

### Requirement: Hires listing
`nearbee hires` SHALL print one line per journaled hire, read only and without the lock: hire id, agent handle, state (open generation n, delivered, frozen, closed accepted, closed cancelled, closed dispute resolved, deleted), the latest run id, spent cost, deadline, and an attention code when the hire needs the operator; it SHALL print no brief text, token or capability link, and `--json` SHALL print the same as JSON.

#### Scenario: Attention
- **WHEN** a hire's apb start lookup was incomplete
- **THEN** its line carries the attention code `start_unresolved`

#### Scenario: A broken journal
- **WHEN** one hire's journal does not parse
- **THEN** that hire's line says so and the other hires are listed

### Requirement: What cleanup deletes
`nearbee cleanup` SHALL take the serve lock and delete, for each eligible hire, only paths Nearbee owns: the hire's worktree (inputs and outputs) and its snapshot directory; it SHALL report apb run artifacts and apb's decision records as not covered, and SHALL journal `deleted` with every path removed, already absent, failed or not covered.

#### Scenario: Partial cleanup
- **WHEN** the worktree is removed and the snapshot directory cannot be removed
- **THEN** the `deleted` record lists the worktree as removed and the snapshot as failed, and the hire stays eligible for the next cleanup

#### Scenario: Removal not yet durable
- **WHEN** the worktree is removed but the directory that held it cannot be synced
- **THEN** the `deleted` record lists the worktree as failed, and the hire stays eligible for the next cleanup

#### Scenario: Retry of a removal not yet durable
- **WHEN** a retry finds the worktree already absent and the directory that held it still cannot be synced
- **THEN** the worktree is listed as failed again, and as absent only once that directory is synced

#### Scenario: Snapshots that cannot be listed
- **WHEN** the hire's journal directory can be traversed but its entries cannot be listed
- **THEN** the `deleted` record lists the snapshots as failed, nothing claims them deleted, and the hire stays eligible until they are removed

#### Scenario: Outside path
- **WHEN** a journal names a worktree outside the apb workspace's `work/` directory
- **THEN** cleanup does not touch it and reports it as refused

### Requirement: Eligibility
A hire SHALL be eligible for cleanup when it closed `accepted` at least `retention.after_acceptance_days` days ago (default 7), when it closed `cancelled` with no delivery, when it was declined at intake, when it closed `cancelled` after a delivery or `dispute_resolved` at least the retention days after that closing was first observed; a frozen (disputed) hire, a hire with an open intent or one with unsettled local work SHALL never be eligible.

#### Scenario: Recently accepted
- **WHEN** a hire was accepted two days ago
- **THEN** cleanup leaves it

#### Scenario: Cancelled after a delivery
- **WHEN** a delivered hire was cancelled during a rework eight days ago
- **THEN** cleanup deletes its data

#### Scenario: Declined at intake
- **WHEN** a hire was declined at intake an hour ago
- **THEN** cleanup deletes its data now

#### Scenario: A late run still running
- **WHEN** a declined hire adopted its late run, which is still running, and no intent of the hire is open
- **THEN** cleanup leaves it until the run's end is recorded

### Requirement: Dry run
`nearbee cleanup --dry-run` SHALL print what would be deleted and not covered, take no lock and change nothing.

#### Scenario: Dry run
- **WHEN** the operator runs `nearbee cleanup --dry-run`
- **THEN** the paths are listed and no file or journal changes
