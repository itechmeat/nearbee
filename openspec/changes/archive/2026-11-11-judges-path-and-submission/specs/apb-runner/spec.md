# Spec Delta

## ADDED Requirements

### Requirement: Instance version
The runner SHALL read the instance version from `GET /api/health`, parse it as a semantic version with prerelease order, and report external-data runs as supported when the version is at least `1.0.0-beta.7`; a missing or unparsable version SHALL be a protocol error.

#### Scenario: Prerelease order
- **WHEN** the instance reports `1.0.0-beta.10`
- **THEN** external-data runs are supported, as they are for `1.0.0`, and not for `1.0.0-beta.6`

#### Scenario: Unparsable version
- **WHEN** the health answer's version is `dev`
- **THEN** the runner returns a protocol error naming the field

### Requirement: External-data runs
A start request SHALL carry `external_data: true` in the body when the caller sets the flag and no `external_data` key when it does not, and run observation SHALL report the run as an external-data run only when the run detail carries `external_data: true`.

#### Scenario: Flag sent
- **WHEN** the caller starts a run with the flag set
- **THEN** the start body carries `"external_data": true`

#### Scenario: Flag not set
- **WHEN** the caller starts a run without the flag
- **THEN** the start body has no `external_data` key

#### Scenario: Flag absent in the detail
- **WHEN** a run detail has no `external_data` field
- **THEN** the run is reported as not an external-data run
