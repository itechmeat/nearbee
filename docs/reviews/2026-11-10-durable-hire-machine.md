# Quality cycle: durable-hire-machine (S3)

Change: `openspec/changes/durable-hire-machine`. Implementers:
GLM-5.3-Flash (ZCode, two sessions) for groups 0 to 4 and the round
machine of group 5; Claude Opus (headless, fresh sessions on two accounts)
for the rest of groups 5 to 9 and 12 and fix rounds 1 to 3; Claude Opus
subagents of the integrator for fix round 4 and the audit fixes, while both
accounts were out of quota. Date: 2026-11-10.

## Steps

| # | Step | Reviewer family | Rounds | Result |
| --- | --- | --- | --- | --- |
| 0 | Artifact review (brainstorm, proposal, design, specs, tasks) | Codex | 2 | 19, then 1 new; all resolved before implementation |
| 1 | Native gate (fmt, clippy `-D warnings`, `cargo test --workspace`, `cargo deny check`, `openspec validate --all --strict`, `node --test` in `examples/x402-hire`) | n/a | after every fix round | green on the final tree: 456 passed, 0 failed, 4 ignored |
| 2 | OCR (OpenCodeReview) | Claude | 1 | 43 raw comments, 4 verified (2 tests that proved nothing, apb down stopping every round, unread agents dropped from the round); all fixed |
| 3 | Cross-family review | Codex | full diff, then 3 full re-reviews, then 1 targeted check | 18 (8 high), 13 (4 high), 9 (5 high), 7 (4 high); then 2 left, both needing a power loss (accepted, see Gaps) |
| 4 | `security-audit` (diff-focused) and `test-audit` (every changed test) | Claude, clean contexts | 1 each | 1 low security finding (rework attachments skipped the listing limits), fixed; 27 test findings (1 high, 12 medium, 14 low), 25 fixed |
| 5 | `/security-review` on the change | Claude | 1 | no findings at high or medium |
| 6 | Final review | Codex | 1 | ready to merge; no issue reachable in normal operation or after a crash |

## Findings that changed the design

1. The one-run slot is derived from every journal on disk, not from the
   rows read this round, and is held by open aborts, unresolved starts and
   non-terminal adopted runs of any generation. An unread or unlistable
   journal holds it. The same predicate decides cleanup eligibility and the
   long-step cut-off.
2. Hires whose rows were not read (a failed read, a revoked token, no
   listing loaded) still get a local pass: start lookup, abort, observation
   and the wall clock. `serve` keeps running while any journal has local
   work.
3. Every unresolved start outcome (none, several, a failed lookup) is
   declined `failed` at the deadline; a hire-wide stray sweep runs whatever
   the execution lookup returned, and ended ambiguous runs are charged with
   their start budget.
4. Progress and download due times, the submit's generation key and the
   delivered decision are journaled, so a restart neither repeats a write
   early nor opens a rework over an unsettled submit.
5. Attachments are written through directory handles that refuse symlinks
   and non-regular files; rework files meet the listing's limits across all
   generations.
6. The run's `brief` is the title plus the description; a rework continues
   only from an adopted run; a manifest with output files is declined.

## Gaps

1. Owner and live tasks open at merge, carried to the owner
   (`docs/stages.md`, end of S3): 10.1 and 10.2 (a killed `serve` resuming a
   live hire, a rework with feedback and a file) and 10.3 after them; S2's
   x402 proof 8.2 and 8.3 (O-5, O-6); the billed Cloud smokes 9.1 (O-3).
   Until 10.1 and 10.2 run, the round machine is proven by the fake market
   and runner only.
2. Two durability gaps are accepted, both needing a power loss together
   with a failed directory sync; they are described in
   `docs/architecture.md`, "Known limitations".
3. Two test-audit findings are left: the sync of an existing `inputs/`
   cannot be observed without a test-only fault hook, and the refused
   HTTP/2 stream through `ApbRunner::start` would need a TLS HTTP/2 test
   server. Two spec scenarios have no test of their own: a dispute redo
   before a restart after a landed submit, and the copy of
   `apb.config_dir` into serve's apb options.
4. The review exceeded the bound of two repair rounds (four, then the audit
   fixes). After round 4 the integrator proposed to the owner to stop full
   rounds, fix what is reachable in normal operation or after a crash, and
   document the rest; with no answer, the integrator applied that rule and
   the final review confirmed it.
