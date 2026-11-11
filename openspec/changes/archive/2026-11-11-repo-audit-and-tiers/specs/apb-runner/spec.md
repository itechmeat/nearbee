# Spec Delta

## ADDED Requirements

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
