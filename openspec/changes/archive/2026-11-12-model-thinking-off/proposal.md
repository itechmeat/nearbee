# Proposal: model thinking off

## Why

Live hires of both example agents cannot finish: the signed DeepSeek V4.1
Flash calls reason past the 240-second completion timeout and never return
an answer (measured 2026-11-12, `brainstorm.md`). This blocks the live demo
(plan items 15 to 17) and every real hire.

## What Changes

- The review request of `nearbee audit review` and the digest request of
  `nearbee digest compose` carry `"chat_template_kwargs": {"thinking": false}`.
- The exact-completion timeout of the Cloud client becomes the time left in
  the 360-second signed-call invocation instead of a fixed 240 seconds.
- `docs/privacy.md` describes the request shape with the new key.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `repo-audit`: the review request turns reasoning off.
- `doc-digest`: the digest request turns reasoning off.
- `signed-call`: the completion wait uses the invocation's remaining bound.

## Impact

`crates/nearbee/src/audit.rs`, `crates/nearbee/src/digest.rs`,
`crates/nearbee-cloud/src/client.rs`, `crates/nearbee/src/signed_call.rs`,
their tests, `docs/privacy.md`. No market or apb change.
