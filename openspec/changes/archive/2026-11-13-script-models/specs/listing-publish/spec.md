# Spec Delta

## ADDED Requirements

### Requirement: Script models field
A listing MAY set `script_models`, a list of `<agent>/<model>` strings kept in order, where `<agent>` is a CLI name of lowercase letters, digits and `-` and `<model>` is non-empty without whitespace (and without a further `/` for `claude` and `codex`). A malformed or duplicated entry SHALL be refused naming it. The list SHALL be recorded with the listing and frozen with each generation's statement snapshot.

#### Scenario: Declared Claude model
- **WHEN** a listing sets `script_models: [claude/claude-opus-5-5]`
- **THEN** loading keeps that entry

#### Scenario: Any agent CLI
- **WHEN** a listing sets `script_models: [claude/claude-opus-5-5, dsh/zai/glm-5.3-flash]`
- **THEN** loading keeps both entries in that order

#### Scenario: Malformed entry
- **WHEN** a listing sets `script_models: [claude-opus-5-5]`
- **THEN** loading fails naming `claude-opus-5-5`

#### Scenario: Duplicate entry
- **WHEN** a listing lists `claude/claude-opus-5-5` twice
- **THEN** loading fails naming it
