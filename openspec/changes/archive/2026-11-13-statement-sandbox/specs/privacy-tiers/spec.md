# Spec Delta

## ADDED Requirements

### Requirement: Sandbox fact
The runner's inspection SHALL report a sandbox fact only when `systemctl --user show nearbee-apb -p ExecStart` shows a `docker run` of an image named `nearbee-sandbox:<tag>` whose arguments include `--read-only`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--dns 127.0.0.1` and `--network <name>`; its hosts SHALL be the sorted, deduplicated names of every `--add-host <name>:<ip>`. A failed read or any other command line SHALL yield no sandbox fact.

#### Scenario: Sandboxed unit
- **WHEN** the unit's ExecStart is the sandbox's `docker run` with three `--add-host` entries
- **THEN** the fact lists those three host names sorted

#### Scenario: Missing limit
- **WHEN** the `docker run` lacks `--cap-drop ALL`
- **THEN** there is no sandbox fact

### Requirement: Facts without a sandbox
Facts without a sandbox fact SHALL serialize exactly as before this change.

#### Scenario: Unchanged fingerprint
- **WHEN** the instance runs without a sandbox
- **THEN** the facts fingerprint equals the fingerprint the same facts had before this change
