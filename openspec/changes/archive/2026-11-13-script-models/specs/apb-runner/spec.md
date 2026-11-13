# Spec Delta

## ADDED Requirements

### Requirement: Script models run param
`script_models` SHALL be a reserved run param: a playbook param of that name SHALL be refused like `direct_models`, and a run of a listing with a non-empty frozen `script_models` SHALL receive it as a JSON list in order; a listing without it SHALL send no such param.

#### Scenario: Param sent
- **WHEN** a hire of a listing with `script_models: [claude/claude-opus-5-5]` starts a run
- **THEN** the run params contain `script_models` = `["claude/claude-opus-5-5"]`

#### Scenario: Param absent
- **WHEN** a hire of a listing without `script_models` starts a run
- **THEN** the run params have no `script_models`
