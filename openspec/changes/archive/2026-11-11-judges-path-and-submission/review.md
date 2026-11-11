# Independent review

## Reviewer

A fresh Claude Opus context that did not write the artifacts, read-only on
the change folder, the repository at `main` (S5) and apb `v1.0.0-beta.7`;
one round, as the owner asked for one review round only. The author
checked each finding against the code and decided every one; there was no
re-review.

## Findings

1. design 5.6, spec `repo-audit`: only a `tee_signature` entry would fail the delivery recheck, which needs the request and response too. Fixed: all four evidence files copied and listed as `digest::EVIDENCE` does.
2. design 5.1: keeping a profile, or the copied `defaults.profile`, makes the profile a usable reference and refuses the attested listing. Fixed: the fast listing names no profile and `playbook.fast.yaml` has no default profile.
3. design 1.5, spec `replay-walkthrough`: the fast listing has no hard limits, so the oversized decline could not happen. Fixed: the decline scenario fails the `github_repo` intake check.
4. design 1.4: the replay's signature is the docs example; its receipt link would not verify. Fixed: the banner and the excerpt say it is synthetic.
5. design 1.5.4: a landed start is read from the journal, so the restart scenario never used the lookup. Fixed: the first start answers start-outcome-unknown, then the restart resolves the intent by `hire_id`.
6. design 5.3, spec `privacy-tiers`: only `direct_models` is a run param. Fixed: the command checks direct models; serve's recheck enforces signed models.
7. design 3.5: the gate came after the market start. Fixed: a second gate at intake declines `rejected` before the market start.
8. design 3.2: semver and the minimum version in the core broke the seam rule. Fixed: `Runner::external_data_supported` returns a bool; the version logic lives in `nearbee-apb`.
9. design 4.1: an agent node with no profile passed the attested check. Fixed: a structural rule refuses any `agent_task` or `prompt` node on an attested listing, with a scenario.
10. design 5.5: an unparsable answer delivered an empty audit. Fixed: the step fails and the hire is declined `failed`.
11. spec `privacy-tiers`: the MODIFIED requirement dropped the route clauses but kept their scenarios. Fixed: those scenarios now end refused by the agent rule; design 4.2 says the fact checks stay for plain statements.
12. scope: group 4 is large three days out. Fixed: fallback in design 5.8 and task 4.4.
13. minor: no invalid-answer error exists. Fixed: `RunnerError::Protocol`.
14. minor: the signature section says "digest call". Fixed: design 5.7, generic words.
15. minor: `finish` has no execution id and does not read the review coverage. Fixed: design 5.6, task 4.1.
16. minor: more than 40 findings unspecified. Fixed: fails the step.
17. minor: `temperature` and `response_format` unverified on Cloud. Fixed: dropped.
18. minor: `NEARBEE_LOG` is read before any subcommand. Fixed: exempted in the spec.
19. minor: task 2.1 hand-built a flagged fixture. Fixed: a real flagged run of a throwaway script-only playbook on Nearbee's instance (it already runs beta.7, checked).
20. minor: task 7.5 named listings without tokens. Fixed: only the fast listing.
21. minor: "Built at NEAR HACKATHON" missing from design 6.3. Fixed.
22. minor: missing scenarios (unreadable version, flag missing on the first run, verbose) and `Runner` implementations to update. Fixed.
23. minor: the premium listing's Claude Code keeps sessions under the owner's home, outside the flag. Fixed: design 3.8 and task 6.2.

## Verdict

Ready for implementation: yes
