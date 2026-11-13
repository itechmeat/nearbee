# Spec Delta

## ADDED Requirements

### Requirement: Script accounts
A listing MAY declare `script_accounts`, a map from an agent CLI named in `script_models` to an account kind; for `claude` the kinds SHALL be `subscription` (the default) and `anthropic-api`. An unknown agent key, a key not used by `script_models` or an unknown kind SHALL be refused naming it.

#### Scenario: API account kept
- **WHEN** a plain listing declares `script_models: [claude/claude-opus-5-5]` and `script_accounts: {claude: anthropic-api}`
- **THEN** the listing is accepted and records the account

#### Scenario: Unknown kind refused
- **WHEN** a listing declares `script_accounts: {claude: team}`
- **THEN** publishing is refused naming `script_accounts` and `team`
