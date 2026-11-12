# Independent review

## Reviewer

A fresh Claude Opus context that did not write the artifacts, read-only on
the change folder and the repository at `main`; one round, as the owner
asked for one review round only. The author checked each finding against
the code and decided every one; there was no re-review.

## Findings

1. design 2.2: giving the completion all the time left could leave no time for the signature fetch, so a paid answer would stay unsigned. Fixed: a 30-second signature reserve inside the 360-second bound, in the design and the signed-call delta, with a scenario.
2. tasks 2.1: there is no fake clock and paused tokio time does not work with wiremock sockets. Fixed: real short durations, the reserve a `Timing` field, four named test cases.
3. design 2.2: plain `Duration` subtraction can panic and leave a false `sending` state. Fixed: saturating subtraction before `sending` is written; nothing left fails while still `preflight`.
4. design 2.1: keeping `CHAT_EXACT_TIMEOUT` as a cap would undo the fix. Fixed: the constant is removed.
5. minor: the timeout text would print an odd remainder. Fixed: it names the invocation bound (design 2.3).
6. minor: the replay builds a review-shaped request too. Fixed: it gets the key (design 1.2, task 1.3).
7. minor: the live check did not require a verified signature. Fixed: design 4 requires `signature.json` and the deliverable.
8. minor: task 3.1 had no verification. Fixed.
9. minor: the delta format was checked and is valid. No change.

Ready for implementation: yes
