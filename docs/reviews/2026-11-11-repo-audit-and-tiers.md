# Quality cycle: repo-audit-and-tiers (S4)

Change: `openspec/changes/repo-audit-and-tiers`. Implementers:
GLM-5.3-Flash (Golem, without builds) for groups 1 to 5 and the first pass
of group 6; Claude Opus (headless, one account) for the rest of group 6,
groups 7 to 10 and the review fixes, after Golem was stopped by a memory
shortage on the machine and then ran out of quota. The integrator built,
tested and fixed compile errors between Golem's groups. Date: 2026-11-11.
The owner asked for one round of checks.

## Steps

| # | Step | Reviewer family | Rounds | Result |
| --- | --- | --- | --- | --- |
| 0 | Brainstorm partner | Codex | 1 | scope narrowed: signed step to S5, embedded JSON instead of file uploads, GitHub snapshot instead of `git clone` |
| 1 | Artifact review | Codex | 1 | 22 findings (3 blockers), all fixed before implementation |
| 2 | Native gate (fmt, clippy `-D warnings`, `cargo test --workspace`, `openspec validate --strict`) | n/a | after every group and the fix round | green on the final tree: 615 passed, 0 failed, 4 ignored |
| 3 | OCR (OpenCodeReview) | Claude | 1 | 27 raw comments, 3 verified; all fixed |
| 4 | Independent code review | Claude Opus, fresh context (Codex was out of its weekly quota, Golem out of quota and the author of half the code) | 1 | 12 findings (3 high, 5 medium, 4 low); all fixed |
| 5 | Live checks | n/a | after the fix round | `nearbee list` of the attested listing passes on the live instance; the premium playbook ran twice on a real repository (`docs/demo-log.md`) |

## Findings that changed the design

1. The attested check requires the `opencode` agent for every executor,
   fallback and tier, not only a `nearai/` model.
2. The apb instance runs with Nearbee's own `XDG_CONFIG_HOME`; the
   inspection treats the routes as unknown when that is not so, when the
   workspace or a parent holds OpenCode configuration, or when
   `/etc/opencode` exists.
3. The `audit` node may write only `audit/**` (one file) and may not
   touch `audit-snapshot/**`; `finish` checks each cited file's hash
   against the fetch record.
4. An inspection that cannot be read holds the listing for the round (no
   intake or start, no decline); only a mismatch on readable facts is
   drift that declines at once.
5. An unknown decisions or compaction setting refuses every tier.
6. The scope question strips control characters and caps the brief at
   32 KiB (found by agent A's live audit of this repository).

## Gaps

1. One review round, by the owner's decision: no re-review of the fixes,
   no separate `security-audit`, `test-audit` or `/security-review` step.
   The fixes were checked by the gate and the live runs only.
2. No cross-family code review: Codex was out of its weekly quota and the
   GLM family wrote part of the code; the code review was Claude (OCR and
   a fresh Opus context).
3. OpenCode 2.2.5 also loads skills and instructions from `~/.claude`,
   `~/.agents` and `.claude/` or `.agents/` above the working directory
   whatever `XDG_CONFIG_HOME` says. Hiding them needs a separate HOME for
   the apb unit, which would also hide the Claude login the premium
   listing runs on. Accepted for v1 and stated in `docs/privacy.md`
   (D-008: a balanced promise without over-engineering).
4. apb `v1.0.0-beta.6` cannot sandbox an OpenCode agent step (`isolation:
   full` refuses it); the agent steps run unsandboxed on the owner's
   machine, which the statement says.
5. The deliverable secret scan is a substring match, so an encoded secret
   passes; documented in `docs/privacy.md`.
6. Not live yet: the fast variant's run (Cloud credits, O-3), the premium
   listing's publish (its draft agent and token, O-2), both listings going
   live (O-6), and a hire through `serve` with the footer and the evidence
   section.
