# Proposal

## Why

Nearbee's privacy story needs a deliverable whose model step carries
proof that a model TEE produced its response, not only a configuration
that says it should. Stage S5 (`docs/stages.md`, plan
items 16 and 17) ships example agent B with a per-response `provider_tee`
signature the buyer verifies in the Verification Center, builds the signed
step of D-008 as a reusable command, and runs the paid agent-to-agent hire
the demo shows.

## What Changes

1. `nearbee signed-call`: one non-streamed Cloud chat completion from exact
   request bytes with its `provider_tee` signature and a matching model
   report, resumable after the completion, never resent; its state lives
   in a per-execution directory, so a rework makes a new signed call.
2. Agent B `doc-digest-private`: `playbooks/doc-digest/` with script nodes
   only (`nearbee digest pages`, `extract`, `classify`, `compose`); PDF,
   PNG and JPEG, at most 10 files and 40 pages; Nearbee renders the
   deliverable from a validated `digest.json`.
3. Listing files gain `direct_models` and `signed_models` (optional, not
   breaking); the tier check, statement, fingerprint and evidence model
   set include them, and each generation freezes them; `NEARAI_CLOUD_URL`,
   `NEARBEE_CLOUD_` and proxy variables in the instance environment make
   attested routes unknown.
4. Serve renders a "Response signature" section with a Verification Center
   receipt link after rechecking hashes and signed text against the
   private snapshot; a mismatch declines `failed`, and a listing with
   `signed_models` declines a run without the signature. The section says
   what the buyer can verify: a model TEE signed a response with the shown
   hash; the request and response bytes stay with the owner.
5. Decision D-026 widens D-008: vision, classify and the digest are direct
   calls from trusted script nodes; only the digest is signed.
6. The paid A2A hire of `repo-audit-fast` with the x402 caller (live,
   owner-gated), recorded in `docs/demo-log.md`.

## Capabilities

### New Capabilities
- `signed-call`: the signed model call, its stored bytes, state and evidence entries.
- `doc-digest`: agent B's inputs, page rendering, extraction, classification, digest contract and deliverable.

### Modified Capabilities
- `listing-publish`: listing files gain `direct_models` and `signed_models`.
- `privacy-tiers`: direct models in the attested check; redirecting variables.
- `data-statement`: direct models in the statement.
- `run-evidence`: direct models in the evidence model set.
- `hire-serving`: delivery renders and rechecks the response signature.

## Impact

1. Crates: `nearbee-cloud` (chat completion with exact bytes, signature,
   privacy classify), `nearbee-core` (tiers, statement, evidence, manifest
   evidence rendering, serve delivery, the new `signature` recheck),
   `nearbee-apb` (the `direct_models` run param, the route rule),
   `nearbee` (`signed-call`, `digest` commands, listing fields). Seam rule holds: Cloud HTTP stays in
   `nearbee-cloud`.
2. No new dependencies expected (`pdftoppm` and ImageMagick are system
   tools, found at runtime; their absence fails the step with a clear
   message).
3. Out of scope: DOCX, per-page signing, verifying signatures in Rust,
   extraction reuse across reworks, the blind tier (D-011), third-party
   hires before apb ships `external_data` (D-023).
