# Brainstorm

## Problem

S5 ships example agent B (`doc-digest-private`): documents and images in,
a digest out, every model call on NEAR AI Cloud Confidential TEE models and
the final digest call carrying a per-response `provider_tee` signature the
buyer checks in the Verification Center. It also builds the signed step of
D-008 as a reusable command and runs the paid agent-to-agent hire of the
demo. The implementer is a Claude Opus coding agent that builds and tests;
about one day.

## Related work

1. `docs/stages.md` S5, plan items 16 and 17; `docs/privacy.md` "Attested,
   signed (contract for S5)".
2. Decisions D-007, D-008, D-011, D-014, D-023, D-025.
3. S4 (archived `2026-11-11-repo-audit-and-tiers`): tier check, statement,
   endpoint evidence, `nearbee audit` commands as trusted script nodes.
4. `examples/x402-hire` (S2): the paid caller, never run with money (O-5).
5. No open pull requests.

## Doubts

Where the key reaches script nodes; idempotency of billed per-page calls;
prompt injection from document text; image limits for the vision model;
how serve treats manifest evidence; where `direct_models` lives; what can
be live with an empty Cloud balance.

## Approaches considered

1. Model calls from script nodes (author; partner agreed): the TEE vision
   model has no tool calls, so vision, classify and the digest are direct
   calls by Nearbee commands in trusted script nodes, never agent steps.
   This widens D-008 (only the digest was a direct call); recorded as
   D-026.
2. `direct_models` in the listing (partner): it feeds the tier check, the
   statement, the fingerprint and the evidence model set; the commands
   refuse any model outside the list.
3. signed-call (author, refined by partner): exact request bytes,
   `x-no-aliasing: true`, `Accept-Encoding: identity`, refuse a compressed
   answer, a model attestation report fetched just before the completion
   (`provider=near`) whose signing address must match the signature's,
   bounded 404 retry of the signature fetch, a state file so a node retry
   resumes after the completion. The completion is never resent once its
   outcome is unknown: the step fails and the hire is declined `failed`
   (a duplicate billed call with a second signature is worse than one
   failed hire).
4. Deliverable rendered by Nearbee (partner): `deliverable.md` is built
   from the validated `digest.json` with escaping, no links or images, and
   every key figure checked to appear in the text of the page it cites;
   the JSON is embedded (output files stay unsupported).
5. Serve rechecks the signature evidence (partner): it recomputes the
   request and response hashes from the private snapshot, rebuilds the
   signed text, and renders a "Response signature" section with a
   Verification Center receipt link; a mismatch declines `failed`.
6. `NEARAI_CLOUD_URL` (partner): a variable that could redirect direct
   calls makes the attested routes unknown, like `OPENCODE_` variables.
7. Image limits (partner): Qwen3-VL has a 16,384-token context, so pages
   are rendered at most 2 megapixels; ImageMagick only with explicit
   `png:`/`jpeg:` input prefixes and resource limits; DOCX is out
   (no converter installed) and leaves `file_types`.
8. PII (partner, author decided): classify output stays private; the
   digest reports PII categories and counts only; "entities" means
   organisations, document types, dates and amounts, never people's names
   or contact details.

## Decision

Build in this order: signed-call; `digest pages`, `extract`, `classify`,
`compose` with the rendered deliverable; `direct_models` through the tier
check, statement, fingerprint and evidence; serve's signature recheck and
section; the agent B playbook and listing; the paid A2A hire as a live
owner-gated step. Cut: per-page signing, extraction reuse across reworks,
DOCX, verifying signatures in Rust (the Verification Center does it).
Over 40 pages, the first 40 are digested and the deliverable says so.

## Rejected

1. Vision as an agent step: the model has no tool calls.
2. `direct_models` in the playbook: the listing is where tiers live and
   the fingerprint is computed.
3. One resend of an unknown completion: two billed calls and two
   signatures for one digest.
4. Agent-written Markdown: document text could inject links or claims.
5. DOCX through a new converter: not installed, not worth a day.

## Open questions

1. Cloud credits (O-3): every live S5 check is billed and the balance is
   empty; asked of the owner with this change.
2. `repo-audit-fast` has never run on Cloud; its live run precedes the
   paid hire, with `repo-audit-premium` as the fallback target.

## Process

Partner: a fresh Claude Opus context with no access to the author's draft
beyond the brief (Codex was out of its weekly quota until 2026-11-16), one
round, as the owner asked.
