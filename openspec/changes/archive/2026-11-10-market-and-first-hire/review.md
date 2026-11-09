# Independent review

## Reviewer

Golem (GLM-5.3-Flash through the DeepSeek Harness), a different model family
from the author (Claude Opus). One round, read-only, against `docs/stages.md`
S2, `docs/reference/market-openapi.json`, the vendored worker and x402 caller
skills, apb at `v1.0.0-beta.6` and the S1 code. No re-review round: every
finding was accepted and the fixes are local wording and scenario additions.

## Findings

1. listing-publish, Materialisation: the refusal for a missing `trial_candidates: off` was unreachable because materialisation always sets it, while `docs/stages.md` asked for a refusal. Fixed: new requirement "Trial setting" refuses a source playbook with another value and an installed version that does not read back with `off`; `docs/stages.md` item 3 reworded to match.
2. hire-serving, Delivery: no behaviour for a refused or outcome-unknown submit. Fixed: requirement "Refused or unknown submit" with two scenarios; task 6.1 tests them.
3. market-client: who resends a timed-out write was ambiguous. Fixed: requirement "Write resends": the client resends start, identical submit and a message at most once, only on an unknown outcome, and returns write outcome unknown otherwise; scenario for exactly two requests.
4. market-client: a repeated decline answered `invalid_state_transition` was counted as landed even when the row had moved on. Fixed: it counts only when a fresh read shows the row cancelled, else refused; scenario "Decline overtaken".
5. tasks: the stage's `X-Market-Delivery` type was not carried. Fixed: task 2.1 adds a `DeliveryId` type and its parse test.
6. `openspec/config.yaml` placed the `Market` trait in `nearbee-market`. Fixed: the seams paragraph now says the traits live in `nearbee-core`.
7. listing-publish: no behaviour at the pricing-plan cap (`409`). Fixed: scenario "Pricing plan cap": publish fails naming the cap and deletes or patches nothing; task 5.4 tests it.
8. hire-serving: the wall-clock margin was unnamed. Fixed: the requirement names 10 minutes before `slaDeadlineAt`, as design 6 does.
9. x402-caller: row selection and "byte for byte after parsing". Fixed: the row whose `network` the caller holds funds on, taken verbatim; deep-equal after JSON parsing.
10. `docs/stages.md` item 4 still described paying the owner's draft agent. Fixed: it now says the agent is live for the test (O-6), since drafts take no x402 calls.
11. tasks: O-1 and O-2 not named. Fixed: the tasks header records that both were provided before this change.

## Verdict

Ready for implementation: yes
