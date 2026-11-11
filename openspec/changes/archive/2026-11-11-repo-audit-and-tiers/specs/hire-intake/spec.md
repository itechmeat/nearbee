# Spec Delta

## MODIFIED Requirements

### Requirement: Scope check
Unless the listing sets `protection: off`, intake SHALL ask `apb decide is` on Nearbee's apb instance, outside any run, whether the brief is a request for the listing's `scope` (its `description` when `scope` is absent), and SHALL decline `rejected` only on a clear no: an answer with probability `p` below 0.2; an answer with `p` from 0.2 to 0.5 is taken as uncertain. The brief in the question SHALL have NUL and every other control character except newline and tab removed, and SHALL be cut at a character boundary to at most 32 KiB followed by a truncation marker.

#### Scenario: Clear no
- **WHEN** the decision provider answers no with `p` 0.05 to "is this a request for an audit of a public repository"
- **THEN** the hire is declined `rejected` with the listing's out-of-scope text

#### Scenario: Uncertain no
- **WHEN** the provider answers no with `p` 0.35
- **THEN** the hire is taken and the intake record says uncertain

#### Scenario: No provider configured
- **WHEN** apb answers `refused: no_provider`
- **THEN** the check is journaled as skipped (`no_provider`), the hire goes on, and the operator is told once per serve process

#### Scenario: Brief passed without a shell
- **WHEN** the brief contains `$(rm -rf ~)` and quotes
- **THEN** the decide process receives it as one argument, no shell interprets it, and nothing runs

#### Scenario: NUL byte and an oversized brief
- **WHEN** the brief contains a NUL byte and is 200 KiB long
- **THEN** the question carries the brief without the NUL, cut to 32 KiB with the marker, and the decide process starts
