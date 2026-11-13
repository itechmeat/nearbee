# Spec Delta

## ADDED Requirements

### Requirement: Script models are plain only
An attested listing with a non-empty `script_models` SHALL be refused, because a script's own CLI call cannot be checked; a plain listing SHALL accept it. Facts with an empty `script_models` SHALL serialize exactly as before this change.

#### Scenario: Attested listing with script models
- **WHEN** an attested listing declares `script_models: [claude/claude-opus-5-5]`
- **THEN** the listing is refused with a reason that names `script_models` and tier plain

#### Scenario: Unchanged fingerprint
- **WHEN** a listing without `script_models` is fingerprinted
- **THEN** the fingerprint equals the one the same facts had before this change
