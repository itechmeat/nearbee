## ADDED Requirements

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
