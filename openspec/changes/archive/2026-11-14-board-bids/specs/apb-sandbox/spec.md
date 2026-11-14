## ADDED Requirements

### Requirement: socat in the image
The sandbox image SHALL install the `socat` package, which Claude Code's Bash sandbox needs to start.

#### Scenario: socat present
- **WHEN** the image is built and `socat -V` runs in it
- **THEN** it exits 0
