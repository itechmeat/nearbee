# Spec Delta

## ADDED Requirements

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
