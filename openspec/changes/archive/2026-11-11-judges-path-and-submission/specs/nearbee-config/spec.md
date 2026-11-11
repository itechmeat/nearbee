# Spec Delta

## ADDED Requirements

### Requirement: apb version in the config check
`nearbee config check` SHALL print the apb instance version as `ok` when it is at least `1.0.0-beta.7` and as `failed`, naming the minimum, when it is lower or cannot be parsed; a failed version makes the command exit non-zero.

#### Scenario: Supported version
- **WHEN** the instance reports `1.0.0-beta.7`
- **THEN** the check prints an ok line with the version

#### Scenario: Old version
- **WHEN** the instance reports `1.0.0-beta.6`
- **THEN** the check prints a failed line naming `1.0.0-beta.7` and exits non-zero
