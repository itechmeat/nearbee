# nearbee-config Specification

## Purpose
Defines how Nearbee loads its configuration and secrets and how an operator checks them before running anything.

## Requirements

### Requirement: Layered configuration
Nearbee SHALL build its configuration from built-in defaults, then the YAML file named by `--config` (default `nearbee.yaml` in the working directory), then a `.env` file next to it, then the process environment, each later layer overriding the earlier; a variable set to an empty value counts as unset in its layer. Relative paths in the file SHALL resolve against the file's directory.

#### Scenario: Process environment wins over .env
- **WHEN** `.env` sets `APB_URL=http://127.0.0.1:1` and the process environment sets `APB_URL=http://127.0.0.1:7321`
- **THEN** the loaded configuration uses `http://127.0.0.1:7321`

#### Scenario: Relative path resolution
- **WHEN** `/srv/nb/nearbee.yaml` sets `state_dir: .nearbee` and Nearbee runs from `/tmp`
- **THEN** the state directory is `/srv/nb/.nearbee`

#### Scenario: Unknown key
- **WHEN** the YAML file contains a key Nearbee does not know
- **THEN** loading fails with an error naming the key and the file

### Requirement: Secrets never leave the process
Secret values (apb API key, NEAR AI Cloud key, NEAR private keys, market tokens) SHALL be held in a type whose debug and display output is redacted, and SHALL never be written to logs, journals, error messages or command output.

#### Scenario: Credentials in the apb URL
- **WHEN** `APB_URL` (or `apb.url` in the file) holds userinfo such as `http://user:key@127.0.0.1:7322`
- **THEN** loading fails with an error naming `APB_URL` (or `apb.url` and the file) and not containing the value

#### Scenario: Credentials behind extra slashes
- **WHEN** `APB_URL` is `http:////user:key@127.0.0.1:7322`, which the HTTP client's URL parser reads as userinfo
- **THEN** loading fails the same way, without the value

#### Scenario: Not an http URL
- **WHEN** `APB_URL` has no scheme (`127.0.0.1:7322/?next=http://a@b`) or a scheme other than `http` and `https`
- **THEN** loading fails with an error naming `APB_URL` that the value must be an absolute http or https URL, not containing the value

#### Scenario: Command output
- **WHEN** a configured secret value appears in anything a command would print (a URL, an error, an apb answer)
- **THEN** every printed line has it replaced by the redaction marker

#### Scenario: Debug output of a loaded config
- **WHEN** a configuration holding `NEARAI_API_KEY=sk-abc` is formatted with debug output
- **THEN** the output contains no `sk-abc`

### Requirement: Config check
`nearbee config check` SHALL load the configuration, report each required and optional variable as present or missing by name only, reach the apb dashboard, resolve the configured apb workspace, and exit non-zero when anything required is missing or unreachable.

#### Scenario: All present
- **WHEN** the config is valid, the dashboard answers and the workspace exists
- **THEN** the command prints one line per check marked ok and exits 0

#### Scenario: Dashboard down
- **WHEN** nothing listens on the configured apb URL
- **THEN** the command prints the apb check as failed with the URL and exits non-zero

#### Scenario: Missing workspace
- **WHEN** the configured workspace path is not among apb's projects
- **THEN** the command fails naming the path and listing the known project paths

### Requirement: Serve limits and retention
The configuration SHALL accept `limits.daily_loss_usd` (decimal string, default `"20"`, at least zero) and `retention.after_acceptance_days` (integer, default 7, at least 1); any other key under `retention` SHALL be refused like every unknown key.

#### Scenario: Defaults
- **WHEN** the file sets neither key
- **THEN** the daily loss limit is USD 20 and the retention is 7 days

#### Scenario: Negative limit
- **WHEN** `limits.daily_loss_usd` is `"-1"`
- **THEN** loading fails naming the key

### Requirement: apb config directory
The configuration SHALL accept `apb.config_dir`, the `APB_CONFIG_DIR` of Nearbee's own apb instance used for every apb CLI call (abort, decide, and the import and trust approval of `nearbee list`), default `~/.local/share/nearbee/apb-config`, with `~` expanded and relative paths resolved against the file's directory.

#### Scenario: Default
- **WHEN** the file does not set `apb.config_dir`
- **THEN** the CLI calls run with `APB_CONFIG_DIR` set to `~/.local/share/nearbee/apb-config` expanded

#### Scenario: Not the everyday instance
- **WHEN** `apb.config_dir` resolves to the operator's default apb config directory (`~/.config/apb`)
- **THEN** loading fails saying Nearbee needs its own apb instance
