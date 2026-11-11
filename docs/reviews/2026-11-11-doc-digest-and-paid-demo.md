# Quality cycle: doc-digest-and-paid-demo (S5)

Change: `openspec/changes/doc-digest-and-paid-demo`. Implementers: Claude
Opus headless sessions on both accounts (artifact fixes, group 1) until
both reached their five-hour limit, then Claude Opus subagents of the
integrator (groups 2 to 6, the review fixes). Date: 2026-11-11. The owner
asked for one round of checks. Codex was out of its weekly quota and
Golem is not used for development (owner 2026-11-11), so every reviewer
was Claude, each in a fresh context.

## Steps

| # | Step | Reviewer | Rounds | Result |
| --- | --- | --- | --- | --- |
| 0 | Brainstorm partner | Claude Opus, fresh context | 1 | direct calls for vision and classify (D-026), `direct_models` in the listing, Nearbee-rendered deliverable, serve signature recheck |
| 1 | Artifact review | Claude Opus, fresh context | 1 | 2 blockers, 14 majors, 12 minors; all decided in `review.md` before implementation |
| 2 | Native gate (fmt, clippy `-D warnings`, `cargo test --workspace`, `cargo deny check`, `openspec validate --strict`) | n/a | after implementation and after the fixes | green: 729 passed, 0 failed, 4 ignored |
| 3 | OCR | Claude | 1 | 15 raw, 1 verified (response text in error messages); fixed |
| 4 | Independent code review | Claude Opus, fresh context | 1 | 3 medium, 5 low; all fixed |

## Findings that changed the design

1. Every free-text value of the digest passes the privacy filter again and
   its spans are redacted before rendering; a figure holding personal data
   is dropped.
2. A signature is accepted when its signer matches at least one model
   attestation of the preflight report (instances share a key); a report
   without signing addresses stops the step before the paid completion.
3. Parse errors never quote document or model text.
4. An unreadable generation snapshot fails the hire closed.

## Gaps

1. One review round and no cross-family reviewer (see above); the fixes
   were checked by the gate only.
2. Nothing billed ran: the Cloud balance is empty (O-3). The signed call,
   extraction, classify spans (offsets are assumed to be characters) and
   the receipt link are tested against mocks and recorded fixtures only.
3. A buyer whose declared attachment sizes understate the real bytes still
   ends `failed` in `digest pages` rather than `rejected` at intake.
4. Not live: agent B's listing and hire (O-2, O-3), the receipt check in
   the Verification Center, the first `repo-audit-fast` run on Cloud and
   the paid A2A hire (O-5, O-6). These are S5's remaining done-when items.
