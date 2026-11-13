# Spec Delta

## ADDED Requirements

### Requirement: Script account wording
When a listing declares `script_accounts: {claude: anthropic-api}`, the statement SHALL name each `claude` entry as the owner's Anthropic API account (API key) instead of the Claude subscription; without the field the statement SHALL use the subscription wording as before, and the fingerprint SHALL be unchanged.

#### Scenario: API key wording
- **WHEN** the statement of a listing with `claude/claude-opus-5-5` and `script_accounts: {claude: anthropic-api}` is generated
- **THEN** it says `claude`, model `claude-opus-5-5`: the owner's Anthropic API account (API key)
