# Process: how a stage goes from plan to main

Every stage of `docs/stages.md` goes through the same cycle. The roles are
fixed; the agents that fill them may change.

## Roles

| Role | Who | Owns |
| --- | --- | --- |
| Owner | the project owner | product decisions, accounts and keys (`docs/owner-checklist.md`), every spend, publish and submission |
| Integrator | the coordinating agent | the OpenSpec artifacts of each stage, the implementer's brief, acceptance of the result against the specs, the quality cycle, the pull request; accountable for what lands on main |
| Implementer | a separate coding agent (since 2026-11-09: Opus subagents of the integrator, so the cross-family review runs on another family and the final review on a third) | the code and tests of one stage, working through `tasks.md` in its own worktree; runs only the targeted tests of the crates it touches |
| Reviewers | agents that did not write the artifact under review | findings only, never edits |

The implementer does not commit, push, open pull requests or start nested
review cycles; the integrator does those after the cycle is clean.

## OpenSpec

The project uses its own schema, `openspec/schemas/nearbee` (selected in
`openspec/config.yaml`), not the default `spec-driven` one. Its artifacts, in
order:

1. `brainstorm.md`: the stage's approach found together with an independent
   agent of another model family; related work checked first (stage entry,
   decisions, open questions, archived changes).
2. `proposal.md`: why, what changes, capabilities, impact; names the stage
   and plan items.
3. `specs/<capability>/spec.md`: behaviour contracts with testable scenarios.
4. `design.md`: how; every external call with timeout, retry and idempotency
   key; every on-disk state with its crash recovery.
5. `tasks.md`: test-first checklist, each task with its verification; owner
   items named in the task that first needs them; ends with the native gate
   and the docs update.
6. `review.md`: independent review of all artifacts by another model family;
   implementation starts only on "Ready for implementation: yes".

Commands: `openspec new change <name>` (the name from `docs/stages.md`),
`openspec status --change <name>`, `openspec instructions <artifact> --change
<name>`, `openspec validate <name> --strict`, `openspec archive <name> --yes`.

Capabilities are named for durable behaviour (`hire-journal`,
`market-client`, `apb-runner`, `listing-tiers`, `result-manifest`, ...), not
for the stage, so later stages modify them with deltas.

## Cycle of one stage

1. **Artifacts.** The integrator writes brainstorm, proposal, specs, design and
   tasks; an independent reviewer reviews them; findings are fixed or rejected
   with a reason (one re-review round at most). Owner questions only for what
   changes the product or the path, with a recommendation.
2. **Brief.** The integrator hands the implementer the change folder, the
   worktree path (off a fresh `main`), the rules of `AGENTS.md`, a time budget,
   and where to write its report.
3. **Implementation.** The implementer works through `tasks.md`, ticking boxes
   as it goes, tests first, docs in the same change. Owner items are requested
   by the integrator at the task that needs them, not before.
4. **Acceptance.** The integrator checks the result against every requirement
   and scenario: reads the diff, reruns the tests, runs the live checks the
   tasks name. Gaps go back to the implementer in the same worktree.
5. **Quality cycle** on the final candidate, in this order, each step covering
   the final head:
   1. Native gate: `cargo fmt --all --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo test --workspace`,
      `openspec validate <change> --strict`.
   2. OCR (OpenCodeReview): high effort on the first round, medium after.
   3. Cross-family review until clean: a reviewer of a different model family
      than the implementer.
   4. `security-audit` focused on the diff and `test-audit` on every added or
      changed test, each in a clean context.
   5. `/security-review` on the range.
   6. Final review by one more family, right before the pull request.
   A fix after any step re-runs the steps the fix touches, then the final
   review again. At most two repair rounds; at the bound the approach is
   reconsidered with the owner. A reviewer without quota is replaced by an
   available agent of another family, named in the report, never skipped.
6. **Report.** The cycle report (every step, its reviewer family, findings
   with dispositions, gaps stated plainly) goes to
   `docs/reviews/<date>-<change>.md`. Raw reviewer output stays outside the
   repository; `.reviews/` is ignored for local copies. Security findings
   with exploit detail never go into public text.
7. **Archive and pull request.** `openspec archive <change> --yes`; the
   archived change and the updated `openspec/specs/` ship in the stage's pull
   request, so `openspec/specs/` always describes `main`. The pull request is
   one commit, rebased on the latest `main` right before the push, with no AI
   attribution in the commit or the body. The integrator merges on a clean
   cycle and green checks without a separate go-ahead (owner, 2026-11-09);
   spends, publishing listings and the submission still need the owner.

The repository is https://github.com/itechmeat/nearbee (public since
2026-11-09); stage pull requests go there and its CI runs the native gate.

## What runs in parallel

Only one stage pull request is open at a time. While stage N is implemented
and reviewed, the integrator writes and reviews the artifacts of stage N+1 in
the main checkout without committing them; they ship in stage N+1's pull
request. Reviewers of one step run in parallel (read-only); one fixer applies
their findings.
