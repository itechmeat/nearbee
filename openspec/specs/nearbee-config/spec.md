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

### Requirement: apb XDG config directory
The configuration SHALL accept `apb.xdg_config_home`, the `XDG_CONFIG_HOME` the unit of Nearbee's own apb instance carries, default `~/.local/share/nearbee/xdg-config`, with `~` expanded and relative paths resolved against the file's directory. The inspection SHALL compare the unit's `XDG_CONFIG_HOME` with it (spec `privacy-tiers`, OpenCode configuration).

#### Scenario: Default
- **WHEN** the file does not set `apb.xdg_config_home` and `HOME` is `/home/u`
- **THEN** the value is `/home/u/.local/share/nearbee/xdg-config`

### Requirement: apb version in the config check
`nearbee config check` SHALL print the apb instance version as `ok` when it is at least `1.0.0-beta.7` and as `failed`, naming the minimum, when it is lower or cannot be parsed; a failed version makes the command exit non-zero.

#### Scenario: Supported version
- **WHEN** the instance reports `1.0.0-beta.7`
- **THEN** the check prints an ok line with the version

#### Scenario: Old version
- **WHEN** the instance reports `1.0.0-beta.6`
- **THEN** the check prints a failed line naming `1.0.0-beta.7` and exits non-zero

### Requirement: Board block
`nearbee.yaml` SHALL accept an optional `board` block with `interval_secs` (60 to 3600, default 300), `pass_budget_secs` (15 to 300, default 90: the time one bidder pass over every listing may take), `owner_verification` (`none`, `verified` or `identity_verified`, default `none`) and `listings`, a map from listing handle to its board settings. An unknown key or an invalid value SHALL be refused at load.

#### Scenario: Interval out of range
- **WHEN** `interval_secs` is 10
- **THEN** the config load fails naming the key

#### Scenario: Pass budget out of range
- **WHEN** `pass_budget_secs` is 5 or 900
- **THEN** the config load fails naming the key

### Requirement: Board listing settings
Each board listing SHALL take `query` (optional text), `kinds` (default `[standard]`, each `standard` or `instant`), `amount_usd` (optional positive decimal string, default the listing price), `sla_seconds` (optional, 600 to 2592000, default the listing SLA), `max_open_bids` (1 to 50, default 3) and `poster_allowlist` (optional list of account UUIDs).

#### Scenario: Defaults
- **WHEN** the block names a listing with only `query: website`
- **THEN** the listing bids on `standard` jobs at its listing price and SLA with at most 3 open bids, every 300 s

#### Scenario: Competition refused
- **WHEN** a listing's `kinds` holds `competition`
- **THEN** the config load fails naming the value

#### Scenario: Bad amount
- **WHEN** `amount_usd` is `"0"` or `"two"`
- **THEN** the config load fails naming the key

#### Scenario: Config check
- **WHEN** `nearbee config check` reads a config with a board block
- **THEN** it prints one line per board listing with its query, kinds, amount and cap
