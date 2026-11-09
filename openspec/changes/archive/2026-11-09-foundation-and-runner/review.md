# Independent review

## Reviewer

Golem (GLM-5.3-Flash through a headless harness; a different model family
from the author), read-only, one round, 2026-11-09. No re-review round: the
only blocker was rejected on evidence and every other finding was applied as
the reviewer proposed.

## Findings

1. Run lookup and `search: true` params do not exist in apb (blocker):
   rejected. The reviewer read a local apb checkout far behind the released
   version. Field search arrived in apb's "run search fields" change, is in
   the installed 1.0.0-beta.6 (`crates/apb-server/src/routes/runs.rs` and
   `crates/apb-core/src/schema.rs` at that tag), and the live dashboard
   answers `GET /api/runs?field=hire_id:x` with `{"not_indexed":533,
   "runs":[],"withheld":0}`.
2. Reserved-param refusal has no scenario (major): fixed, scenario and test
   added.
3. Workspace-pinning scenario has no fixture (major): fixed, second-workspace
   playbook fixture and test added to 5.3 and 5.4.
4. `nearbee runs find` has no spec (major): fixed, requirement "Run find
   command" with output and exit codes 0, 1, 2.
5. `docs/apb.md` field-search statements are wrong (major): rejected, same
   evidence as finding 1; task 5.8 still verifies them against fixtures.
6. Brainstorm wording on terminal states (minor): fixed.
7. Architecture config example has another port and key (minor): fixed in
   task 8.1.
8. `rust_decimal` missing from the proposal (minor): fixed, with `uuid`.
9. Markdown limit hardcoded in the spec (minor): fixed, configurable with the
   default named.
10. Duplicate paths and Markdown location unspecified (minor): fixed, two
    scenarios.
11. `safe_id` uniqueness and newer `schema_version` unspecified (minor):
    fixed, two scenarios; a newer version is refused, not read tolerantly.
12. Decimal versus apb's float budget (minor): fixed, design note.
13. Queued 200 start and launch-gate refusal (minor): fixed, two scenarios
    and a design note.
14. Running-state fixture is timing-fragile (minor): fixed, a skipped capture
    is reported instead of retried.

Also added from the reviewer's line of thought: a design risk that
`not_indexed` counts old runs, so lookups in a workspace with history are
never complete; the dedicated workspace avoids it.

## Questions for the owner

None.

## Verdict

Ready for implementation: yes
