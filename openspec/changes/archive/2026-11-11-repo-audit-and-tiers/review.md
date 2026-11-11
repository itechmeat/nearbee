# Independent review

## Reviewer

Codex (GPT family), read-only on the change folder, the repository and
apb's source at `v1.0.0-beta.6`; one round, as the owner asked for one
review round only. The author (Claude) applied the fixes below and decided
every finding; there was no re-review.

## Findings

1. design 2: profile references carry a scope and nodes an interactive flag. Fixed: `ProfileRef {name, scope}`, scoped lookup, `interactive`, tests in 2.2 and 4.1.
2. design 2: OpenCode overrides can supersede `opencode.json`. Fixed: an `OPENCODE_` variable in the instance environment makes routes unknown; the supported launch is the script's unit; documented in 10.1. Other undeclared egress stays a stated limitation.
3. design 2, 4: sorting destroyed fallback and tier order. Fixed: only unordered collections are sorted; tests for fallback swap and tier reorder.
4. design 4, tasks 6: drift was checked after intake could send the brief. Fixed: a drift pass before intake each round; drifted hires get no scope check or start; test with zero scope calls.
5. spec `data-statement`: incomplete records and retention changes passed. Fixed: serveable-record rule and `drift` comparing the re-rendered statement.
6. design 5, 6: the evidence model set was not frozen; orphan snapshots reused blindly. Fixed: `evidence_models` in the snapshot; reuse only on matching fingerprint and hash.
7. design 5, spec `run-evidence`: silent footer downgrade and refetched start evidence. Fixed: legacy only without `statement_sha256` in the intent; fail closed otherwise; start fetches only before the intent.
8. design 7: attempt records were lost before recovery. Fixed: persisted in the terminal `run_observed` record; `Reported` none, one or mixed; requested model optional.
9. design 6: retry timing contradicted the bound and fetches bypassed scheduling. Fixed: 2 attempts within 20 s; evidence is a pass 3 long step, one fetch per round.
10. design 6: start evidence ran after the market start; decline at the fourth round; fixed reasons. Fixed: preflight before the market start; decline on the third failure; `waiting_decline` takes reason and attention; tests for unstarted, owner-started, rework, redo.
11. design 6: slash names and unspecified events. Fixed: flat names with a model hash; typed `EvidenceCollected` and `EvidenceFailed`; crash between file and record covered.
12. spec `repo-audit`: snapshot publication not crash-safe. Fixed: one staging directory with metadata, synced, renamed; reuse only a parsed snapshot of the same repository.
13. spec `repo-audit`: invalid briefs found only after start. Fixed: `intake_check: github_repo` declines `rejected` before any start; whole-token scan.
14. spec `repo-audit`: extraction accounting and filesystem edges. Fixed: decompressed cap over everything read, deadline during decoding, fresh staging without following links, duplicates and rename collisions fail, truncated data fails.
15. spec `repo-audit`: candidate validation ambiguities. Fixed: exact version, all fields, scalar-value lengths, non-empty contiguous excerpt, files from the inventory, fence `max(3, n + 1)`.
16. spec `repo-audit`: finish could follow links or leave a stale manifest. Fixed: bounded no-follow reads, `result.json` removed first and published last.
17. design 8: `run.yaml` parsing outside `nearbee-apb` broke the seam. Fixed: `nearbee-apb::run_params::from_env`.
18. design 6, 8: a second runtime and an environment-only key. Fixed: async on the existing runtime with `spawn_blocking`; key from `Config.nearai_api_key`; plain serving needs no key.
19. tasks: groups could not compile alone. Fixed: listing files and initializers in group 1; trait methods with every implementation in group 4; `RunDetail` initializers with `attempts`.
20. tasks: synthetic fixtures broke the seam-test rule. Fixed: task 3.3 records live apb responses before group 4; task 11.5 records a live attestation response; synthetic only for errors.
21. tasks: premium credentials (O-2) needed before its publish. Fixed: 11.3 is its own task waiting on O-2; the rest of the live checks do not depend on it.
22. spec `data-statement`: plain statements omitted connectors and decisions. Fixed: "Other channels" section for both tiers with the not-validated sentence; test.

## Verdict

Ready for implementation: yes
