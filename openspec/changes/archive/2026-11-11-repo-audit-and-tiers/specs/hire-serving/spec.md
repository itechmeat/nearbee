# Spec Delta

## MODIFIED Requirements

### Requirement: Delivery
On a succeeded run serve SHALL validate the manifest, append the generation's evidence section (attested hires) and data statement, check the assembled text against the Markdown limit and the secret scan, store it in the generation's snapshot and submit it; on a failed, aborted or timed-out run, an invalid manifest, or a manifest that lists output files, it SHALL decline with `failed` and a short reason; a run report or summary SHALL never be submitted.

#### Scenario: Invalid manifest
- **WHEN** the run succeeded but its manifest fails validation
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Manifest with output files
- **WHEN** the run's manifest lists files besides the Markdown
- **THEN** serve declines with `failed`, submits nothing, and tells the operator that output files are not supported yet

#### Scenario: Footer pushes the deliverable over the limit
- **WHEN** the playbook's Markdown fits the limit but with the statement appended it does not
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Recovered submit
- **WHEN** serve restarts after the submission snapshot was written
- **THEN** it resends the snapshotted bytes, statement and evidence included, without assembling them again

## ADDED Requirements

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
