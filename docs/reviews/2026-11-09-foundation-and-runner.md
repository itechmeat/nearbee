# Quality cycle: foundation-and-runner (S1)

Change: `openspec/changes/foundation-and-runner`. Implementer: Claude Opus
(headless). Date: 2026-11-09.

## Steps

| # | Step | Reviewer family | Rounds | Result |
| --- | --- | --- | --- | --- |
| 1 | Native gate (fmt, clippy `-D warnings`, `cargo test --workspace`, `openspec validate --all --strict`) | n/a | after every fix round | green on the final head, 125 tests, 3 live tests ignored |
| 2 | OCR (OpenCodeReview) | Claude | high, then medium on each fix commit (3) | 7 findings (1 high, 6 medium), then 1 medium, then 1 low; all fixed |
| 3 | Cross-family review | GLM | full diff, then the fix commit | 5 minor, then 2 minor; all fixed |
| 4 | `security-audit` (diff-focused) and `test-audit` (every changed test) | Claude, clean contexts | 1 each | 2 low plus 1 check; 16 test findings incl. one confirmed flaky test; all fixed |
| 5 | `/security-review` on `main..HEAD` | Claude | 1 | no findings |
| 6 | Final review | Codex | full diff, then each fix commit (3) | 6 findings (3 major), then 2 (1 major), then none |

## Findings that changed the design

1. A run start must never be sent twice: a 3xx answer and an unusable run id
   are an unknown outcome, and the HTTP client's own transport retries
   (HTTP/2 refused streams) are disabled.
2. Manifest containment resolves every path component from an open
   directory descriptor without following links (a parent directory swapped
   for a link after a path walk escaped `out/`), and opens the leaf without
   blocking (a FIFO swapped in could hang validation).
3. The apb URL is parsed with the same parser as the HTTP client and refused
   with credentials or a non-http scheme; every CLI line is redacted.
4. apb may run a candidate version instead of the current one under its
   default trial policy; market listings must declare `trial_candidates:
   off` (`docs/apb.md`).

## Gaps

1. Task 7.3 (two billed NEAR AI Cloud runs) moved to S2 by the owner: the
   Cloud balance is empty; billed launches on Nearbee's apb instance need the
   owner's go-ahead.
2. The review rounds exceeded the bound of two repair rounds (four in all);
   each later round came from a reviewer seeing the code for the first time
   or from a fix of the previous round, never a repeated finding, so the
   approach was not reconsidered.
