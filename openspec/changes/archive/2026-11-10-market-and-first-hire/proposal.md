# Proposal: market and first hire

## Why

Stage S2 of `docs/stages.md` (plan items 6 and 7, plus the billed Cloud runs
carried from S1). Nearbee can drive apb but cannot yet talk to the market.
S2 makes the first end-to-end hire work and proves the x402 payment path,
the second riskiest outside dependency. See `brainstorm.md`.

## What Changes

1. `cargo deny` (advisories, licenses) in the native gate and CI.
2. `nearbee-market`: `NearAgentMarket` implementing the `Market` trait
   (defined in `nearbee-core`): assignments in all statuses with paging,
   start, messages with `clientRequestId`, progress, submit, decline, agent
   read and patch, pricing plans, the file upload trio; recorded real
   responses as fixtures.
3. Nearbee-owned agent tokens: `nearbee agent add <handle>` and a
   credentials file outside the repository (D-022).
4. `nearbee list <listing.yaml>`: materialise the source playbook with
   `trial_candidates: off`, patch the draft agent, set its default per-call
   price; never change `listing_status`.
5. `nearbee serve`: poll, take first-time hires, run the listing's playbook
   through `ApbRunner`, validate the manifest, submit its Markdown, decline
   on failure; journal intents before every external write (the journal
   writer moves forward from S3).
6. `examples/x402-hire`: a `near-api-js` caller that signs a NEP-366
   delegate for a 402 row, verifies it locally, and pays an agent; the
   no-spend proof uses a rejected hire.
7. The first end-to-end hire and the x402 proof recorded in
   `docs/demo-log.md`; the two billed Cloud smoke runs from S1 recorded.

## Capabilities

### New Capabilities

- `market-client`: what Nearbee requires from the market's worker API and
  how it calls it (paging, retries, idempotency, errors).
- `agent-credentials`: how agent tokens are added, stored and read.
- `listing-publish`: how a listing file becomes a draft market agent.
- `hire-serving`: how `serve` takes, runs and delivers first-time hires.
- `x402-caller`: what the demo caller does and proves.

### Modified Capabilities

- `hire-journal`: adds writing (append with fsync, sequence, intents before
  writes).

## Impact

Crates: `nearbee-core` (Market trait, listing types, journal writer, serve
core logic against the traits), `nearbee-market` (new implementation),
`nearbee` (commands `agent add`, `list`, `serve`), `examples/x402-hire`
(Node, outside the cargo workspace). Seams hold: serve logic in core uses
only `Market` and `Runner`. New dependencies: none beyond S1's for Rust
except a no-echo stdin reader if needed; Node `near-api-js` v7 for the
example. Outside systems: the market (owner's draft agent, owner's own
hires), NEAR mainnet RPC (read for the example), apb instance.

## Out of scope

Rework, dispute redo and questions on submitted work (logged, left alone);
intake decisions and limits; attachments download; lock file, recovery
table, cancellation and refund handling (S3); tiers and evidence (S4);
settled payments (the paid demo hire is S5); webhooks (D-011).
