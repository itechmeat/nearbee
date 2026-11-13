# Proposal: nested scripts

## Why

Publishing drops script files in subdirectories of `scripts/`, so an agent
with a script library or prompt files breaks once listed.

## What Changes

- `publish.rs` collects every regular file under `scripts/`, recursively,
  as a path relative to `scripts/` with forward slashes, sorted; a symlink
  under `scripts/` refuses the publish.
- Docs: `docs/apb.md` (the publish install) names the rule.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `listing-publish`: script collection.

## Impact

`crates/nearbee/src/publish.rs`, its tests, `docs/apb.md`.
