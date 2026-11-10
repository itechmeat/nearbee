# Independent review

## Reviewer

Codex (OpenAI family), read-only on the repository, the vendored market
OpenAPI and apb's source at `v1.0.0-beta.6`; it was also the brainstorm
partner. Two rounds: 19 findings in round 1, then a re-review that marked
12 fixed, 6 partly fixed, 1 not fixed and added 1 new finding. The author
(Claude family) resolved what remained after round 2 as listed. The
implementer will be a GLM model, so no reviewer shares its family.

## Findings

1. design, ambiguous apb start: a complete empty lookup cannot prove a lost start will not land. Fixed: no second start for a generation with an unresolved start; it keeps the slot, is declined `failed` at the deadline and is searched until found (stray abort).
2. design, a lost submit answer followed by `submitted`, rework, redo or acceptance would be resent into the next generation. Fixed: a pending submit is settled against the row first (`submittedAt` or a changed generation key, which also covers a redo's `submittedAt: null`). The client's one immediate identical resend inside the same call (S2 Write resends) is kept: rejected as a risk, it happens within seconds while the row still awaits that submission.
3. design, `apb decide` outside a run has side effects (a log line, a possibly paid call). Fixed: `scope_check` intent; an interrupted check is skipped, never asked again; brainstorm corrected (the log holds digests and metadata).
4. market-client, the full thread does not say which party wrote a message. Fixed: no sender mapping; the thread is read only to recover the full body of the latest buyer message matched by `createdAt` and its preview.
5. design and intake, a partial reported cost would leave the budget open. Fixed: completeness from apb's attempt counters; missing or incomplete cost counts as the run's whole budget, in the per-hire and the daily sums.
6. design, terminal observation and cost in separate records. Fixed: the terminal `run_observed` carries the cost; strays use the same path.
7. design, long steps block the round. Fixed: at most one long step per round, after every wall clock is checked, cut off at the earliest deadline of any active hire and retried later.
8. market-client, the S2 progress repeat requirement contradicted the new due-time rule. Fixed: full MODIFIED block.
9. design, a closed hire could strand an active run or an unresolved start. Fixed: requirement Local work after the market closes.
10. design, rework starts and unreadable journals were outside the one-slot gate. Fixed: every apb start needs the slot; unreadable journals hold it.
11. design, S2 pending intents have no snapshot. Fixed: running and terminal S2 hires continue normally; open start or submit intents get `s2_pending` with an operator procedure in `docs/architecture.md`; fixtures for every shape in task 1.4.
12. tasks, the live playbook ignores `feedback` and `inputs`. Fixed: task 8.3, and task 10.2 checks the delivered note reflects the request.
13. attachments, names collide across reworks. Fixed: hire-wide immutable indices.
14. listing-publish, the version was derived from YAML only. Fixed: task 0.5 covers scripts and executable flags.
15. tasks, interface changes before their callers. Fixed: callers adapt in the same task; the fixed budget goes with its replacement in 6.3.
16. tasks, docs collected in the last group. Fixed: docs per group.
17. listing-publish, bio missing from the report, inherited SLA overstated. Fixed.
18. retention, cancelled after a delivery had no anchor. Fixed.
19. listing, the cent threshold differed from the formula. Fixed: the formula, with `0.26` refused and `0.264` accepted.
20. hire-serving (round 2), rework ran with truncated or missing feedback. Fixed: full body from the thread; without reliable buyer feedback the rework is declined `failed` with `feedback_unavailable`.

## Verdict

Ready for implementation: yes
