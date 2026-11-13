# Proposal: script models

## Why

A playbook's script steps may call a model through the owner's agent CLI
(ux-flow runs `claude -p` on the owner's Claude subscription). Nearbee names
models in the data statement only from the profiles of agent steps and from
`direct_models` (NEAR AI Cloud), so such a call is invisible: the statement
would not say that the buyer's data reaches Anthropic. The sandbox image has
no Claude Code CLI either, so the call cannot run there.

## What Changes

- A listing may declare `script_models`: the `<agent>/<model>` pairs its
  script steps call through the owner's agent CLIs. Plain tier only; an
  attested listing with any entry is refused.
- The data statement gets a "Script model calls" section naming each
  declared pair with the existing per-agent wording, marked as declared by
  the owner; the fingerprint covers the list and is unchanged without it.
- Runs get a reserved `script_models` param (JSON list), like
  `direct_models`, so a script refuses a model the listing did not declare.
- The sandbox image carries the Claude Code CLI at a pinned version, with
  its auto-updater and non-essential traffic off.
- Docs: the listing field, the statement wording, passing
  `CLAUDE_CODE_OAUTH_TOKEN` through `NEARBEE_APB_ENV_VARS`, and the owner's
  rule that a script whose model sign-in fails exits non-zero so the hire is
  declined `failed` with a full refund (never a fallback provider).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `listing-publish`: `script_models` field.
- `data-statement`: script model calls.
- `privacy-tiers`: script models are plain-only.
- `apb-runner`: the `script_models` run param.
- `apb-sandbox`: Claude Code CLI in the image.

## Impact

`crates/nearbee-core/src/listing.rs`, `tiers.rs`, `statement.rs`,
`runner.rs`, `serve.rs`; `crates/nearbee-apb/src/lib.rs`; `crates/nearbee`
where `direct_models` is threaded (publish, serve, replay); tests;
`sandbox/Dockerfile`; `docs/privacy.md`, `docs/apb.md`,
`docs/architecture.md` if it lists listing fields. Listings without the
field keep their fingerprints.
