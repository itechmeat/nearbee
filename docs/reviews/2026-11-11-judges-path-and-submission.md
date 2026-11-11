# Quality cycle: judges-path-and-submission (S6)

Change: `openspec/changes/judges-path-and-submission`. Implementers: one
Claude Opus headless session on the second account (group 1) until both
accounts reached their five-hour limit, then Claude Opus subagents of the
integrator (groups 2 to 6, the review fixes). Date: 2026-11-11. The owner
asked for one round of checks. Codex was out of its weekly quota and
Golem is not used for development (owner 2026-11-11), so every reviewer
was Claude, each in a fresh context.

## Steps

| # | Step | Reviewer | Rounds | Result |
| --- | --- | --- | --- | --- |
| 0 | Brainstorm partner | Claude Opus, fresh context | 1 | replay at `serve::round` with own fakes, the journal clock, the beta.7 compaction refusal, the `~/.agents` source of OpenCode |
| 1 | Artifact review | Claude Opus, fresh context | 1 | 2 blockers, 10 majors, 11 minors; all decided in `review.md` before implementation |
| 2 | Native gate (fmt, clippy `-D warnings`, `cargo test --workspace`, `cargo deny check`, `openspec validate --all --strict`, `nearbee replay`) | n/a | after implementation and after the fixes | green: 775 passed, 0 failed; 20 of 20 specs; replay 5 of 5 |
| 3 | OCR | Claude | 1 | 9 raw, 2 verified (unbounded omitted-file list, repeated operator notice at the run gate); fixed |
| 4 | Independent code review (correctness, security, test quality) | Claude Opus, fresh context | 1 | 0 critical, 0 high, 4 medium, 5 low; 9 fixed, 1 answered from apb's documentation |
| 5 | Clean-clone run of README step 1 | integrator | 1 | a fresh clone, `cargo run -p nearbee -- replay`: 5 of 5 ok, 16.35 s on 16 cores with the crates already downloaded |

## Findings that changed the design

1. apb `1.0.0-beta.7` made every listing refusable: compaction was known
   off for beta.6 only. beta.7 was re-read (no start surface sets
   `context_max_bytes`) and added; versions compare as semver.
2. OpenCode 2.2.5 runs `opencode run` inside a shared background service
   with the starting user's configuration and MCP servers, and still loads
   `~/.agents/skills` with `--standalone`; apb starts it without
   `--standalone`. Attested listings now refuse any agent (D-027), and
   agent A's fast listing became a signed DeepSeek review by Nearbee's own
   command.
3. A version gate sits before the market start as well as before every run
   start, so an old apb declines `rejected` inside the first hour instead
   of failing later.
4. The external-data check applies only to runs whose start intent
   recorded the flag, so the upgrade does not abort hires already running.
5. The review pack skips a file that does not fit and keeps packing,
   quotes file paths and numbers the brief's lines, so neither a
   repository nor a buyer can close a data block in the prompt.

## Gaps

1. One review round and no cross-family reviewer (see above).
2. Nothing billed ran: the Cloud balance is empty (O-3). The fast review
   call and its receipt are tested against recorded fixtures only.
3. Continued runs carrying the flag were not captured live; apb's
   `docs/EXTERNAL-DATA.md` says a `continued_from` run of a flagged run is
   flagged, and Nearbee sends the flag on every start anyway.
4. The owner's shared OpenCode service is unchanged; Nearbee only stops
   relying on it. Plain listings on OpenCode would inherit it; none ships.
5. Not live: relisting the fast listing (task 7.5, after the merge), the
   first live fast run, agent B's listing (O-2), the paid hire (O-5, O-6).
