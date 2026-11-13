# Proposal: review quality

## Why

A paid fast audit of a mid-size repository came back empty: the model
gave up on a 213k-token pack (`brainstorm.md`).

## What Changes

- `REVIEW_REQUEST_CAP` 768 KiB to 256 KiB.
- The review and digest requests carry `response_format: {"type": "json_object"}`.
- `docs/privacy.md` and `docs/showcase-agents.md` name the new cap and key.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `repo-audit`: review size cap and review call.
- `doc-digest`: digest call.

## Impact

`crates/nearbee/src/audit.rs`, `crates/nearbee/src/digest.rs`, the replay's
review-shaped request, their tests, docs.
