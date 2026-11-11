# Spec Delta

## ADDED Requirements

### Requirement: apb XDG config directory
The configuration SHALL accept `apb.xdg_config_home`, the `XDG_CONFIG_HOME` the unit of Nearbee's own apb instance carries, default `~/.local/share/nearbee/xdg-config`, with `~` expanded and relative paths resolved against the file's directory. The inspection SHALL compare the unit's `XDG_CONFIG_HOME` with it (spec `privacy-tiers`, OpenCode configuration).

#### Scenario: Default
- **WHEN** the file does not set `apb.xdg_config_home` and `HOME` is `/home/u`
- **THEN** the value is `/home/u/.local/share/nearbee/xdg-config`
