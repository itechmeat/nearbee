# Quality cycle: market-and-first-hire (S2)

Change: `openspec/changes/market-and-first-hire`. Implementer: GLM-5.3-Flash
(Golem, headless) for the implementation and fix rounds 1 to 4; Claude Opus
(headless, a fresh session) for fix rounds 5 and 6 while Golem was out of
quota. Date: 2026-11-10.

## Steps

| # | Step | Reviewer family | Rounds | Result |
| --- | --- | --- | --- | --- |
| 0 | Artifact review | GLM | 1 | 4 major, 7 minor; all fixed before implementation |
| 1 | Native gate (fmt, clippy `-D warnings`, `cargo test --workspace`, `cargo deny check`, `openspec validate --all --strict`, `npm test` in `examples/x402-hire`) | n/a | after every fix round | green on the final head |
| 2 | OCR (OpenCodeReview) | Claude | high, then medium | 8 high and about 30 medium, then 2 high and 6 medium; all fixed |
| 3 | Cross-family acceptance and closure review | Claude | full diff, then each fix round (3) | 1 critical, 9 major, 9 minor; then 4 new from fixes; then 4 medium in the rewritten serve state machine; then clean at medium and above |
| 4 | `security-audit` (diff-focused) and `test-audit` (every changed test) | Claude, clean contexts | 1 each | 9 security findings; 27 test findings incl. a critical (wrong NEP-366 signature hidden by a self-referential test); all fixed |
| 5 | `/security-review` on `main..HEAD` | Claude | 1 | no findings |
| 6 | Final review | Codex | full diff, then each fix round | 3 high (every market wire type wrong) and 1 medium; then 2 medium and 1 low; then clean |

## Findings that changed the design

1. The market client's wire types did not match the API: assignments come in
   an envelope of nested items and every agent and pricing-plan field is
   snake_case. Wiremock mocks had been written to the code, so 250 tests
   passed. A contract test now builds instances from
   `docs/reference/market-openapi.json` and every mock body is schema-shaped.
2. Serve's per-hire state is one function deriving the hire's state from its
   journal, with one transition per state; ad-hoc intent checks lost hires on
   a restart, declined twice, or built a second deliverable.
3. A hire runs the playbook version that was published when it started; the
   version and digest are journaled per hire and enforced through recovery.
4. The buyer's brief reaches the playbook only through its untrusted `brief`
   parameter; the deliverable is scanned for tokens and secrets before
   submit; decline reasons sent to buyers are fixed strings.
5. The x402 caller signs with near-api-js's NEP-366 helpers, checks a full
   access key and its nonce on chain, and refuses quotes above `--max-usd`,
   unknown asset contracts and windows above 7 days + 600 s.
6. A draft agent is invisible to every lookup its token can make, so
   `nearbee agent add` takes `--agent-id` from the owner for drafts.

## Gaps

1. Owner and live tasks open at merge, carried to the start of S3
   (`docs/stages.md`): 3.4 recorded fixtures (needs the draft's agent id),
   the real publish of 5.4, the owner's hires 7.1 and 7.2, the x402 proof
   8.2 and 8.3, the billed Cloud smokes 9.1. Until 3.4 runs, the wire shapes
   are verified against the OpenAPI only, not against recorded answers.
2. The review rounds exceeded the bound of two repair rounds (six in all).
   The owner asked for the stage to be driven to the end without
   confirmations; the approach was changed once (serve resume rewritten as an
   explicit state machine after round 2) instead of being reconsidered with
   the owner.
3. The exact `409` code for the pricing-plan cap and the trust approve and
   revoke flags against a real apb are checked against the docs and apb
   source only.
