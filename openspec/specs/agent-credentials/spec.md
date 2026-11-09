# agent-credentials Specification

## Purpose
Defines how Nearbee holds market agent tokens so that no token lives in the repository, the config or `.env`.

## Requirements

### Requirement: Adding an agent
`nearbee agent add <handle> [--agent-id <uuid>]` SHALL read the token from standard input (never from arguments or the environment), resolve the agent id from the public agent record for the handle, prove the token with an authenticated assignments read, and store `{agent_id, token}` under the handle. When the public record answers 404, the agent id SHALL come from the operator as a `--agent-id` UUID.

#### Scenario: Handle not publicly visible without an agent id
- **WHEN** the public record for the handle answers 404 (a draft agent) and no `--agent-id` is given
- **THEN** the command fails telling the operator to copy the agent id from the agent's settings page in the market web app and pass `--agent-id`, and stores nothing

#### Scenario: Agent id taken from the operator
- **WHEN** the public record answers 404 (a draft agent is invisible to every lookup its token can make) and `--agent-id` is a UUID
- **THEN** the command uses that agent id, the output says the id was taken from the operator, and the assignments read proves the token before anything is stored

#### Scenario: Agent id that is not a UUID
- **WHEN** `--agent-id` is not a UUID
- **THEN** the command fails saying what `--agent-id` must hold, makes no request and stores nothing

#### Scenario: Agent id differs from the record
- **WHEN** the public record exists and `--agent-id` differs from the agent id the market returns
- **THEN** the command refuses naming both ids and stores nothing

#### Scenario: Wrong token
- **WHEN** the handle resolves but the assignments read answers 401
- **THEN** the command fails saying the token was refused and stores nothing

#### Scenario: Output
- **WHEN** an agent is added
- **THEN** the command prints the handle, the agent id and the token only as `aat_...` with its last four characters

### Requirement: Credentials file
Tokens SHALL be stored in `$XDG_CONFIG_HOME/nearbee/credentials` (default `~/.config/nearbee/credentials`), a JSON object keyed by handle, written atomically with mode 0600. Reading SHALL refuse a file whose mode grants any access to group or others.

#### Scenario: Wider mode
- **WHEN** the file has mode 0644
- **THEN** every command that needs a token fails naming the file and the expected mode, without reading tokens

#### Scenario: Replace a token
- **WHEN** `agent add` runs for a handle already stored
- **THEN** the new token replaces the old one and other handles are unchanged

### Requirement: Config names agents by handle
Configuration and listing files SHALL name agents by handle only; a token-like value (`aat_` followed by hex) in them SHALL be refused at load.

#### Scenario: Token in a listing file
- **WHEN** a listing file contains an `aat_` token
- **THEN** loading fails without printing the value
