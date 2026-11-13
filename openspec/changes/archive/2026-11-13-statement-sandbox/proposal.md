# Proposal: statement sandbox

## Why

The data statement says the steps run without a sandbox even when they run
in the `apb-sandbox` container; buyers read an outdated limit.

## What Changes

- The runner's inspection reads the unit's `ExecStart` and reports a
  sandbox fact with the allowed hosts when the argv is the sandbox.
- The statement's "Where the run happens" section names the container, its
  limits and the allowed hosts when the fact is present.
- `docs/privacy.md` describes the new wording.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `data-statement`: "Where the run happens".

## Impact

`crates/nearbee-core/src/tiers.rs` (`ExecutionFacts`),
`crates/nearbee-core/src/statement.rs`, `crates/nearbee-apb/src/inspect.rs`,
tests, `docs/privacy.md`. A listing published from a sandboxed instance gets
a new fingerprint, so `nearbee list` must run again and the owner pastes the
new statement.
