# Spec Delta

## ADDED Requirements

### Requirement: Script models in the statement
When the listing declares `script_models`, the statement SHALL contain a "Script model calls" section saying the script steps call these models through the owner's agent CLIs as declared by the owner and not observed by Nearbee, and SHALL name each entry with the same per-agent wording as the Models section; the fingerprint SHALL cover the list in order. Without the field the statement SHALL be unchanged.

#### Scenario: ux-flow statement
- **WHEN** the statement of a plain listing with `script_models: [claude/claude-opus-5-5]` is generated
- **THEN** it says the scripts call `claude`, model `claude-opus-5-5`, the owner's Claude subscription (Anthropic), declared by the owner

#### Scenario: DeepSeek Harness reviewer
- **WHEN** a plain listing declares `dsh/zai/glm-5.3-flash`
- **THEN** the statement names `dsh` (DeepSeek Harness), model `zai/glm-5.3-flash`, through the owner's own provider account `zai`

#### Scenario: No script models
- **WHEN** a listing without `script_models` gets its statement
- **THEN** the text is byte for byte the text before this change
