# Tasks

## 1. Reasoning off

- [x] 1.1 Test first: the review request JSON has exactly the keys `model`, `stream`, `max_tokens`, `chat_template_kwargs`, `messages`, and `chat_template_kwargs == {"thinking": false}`. Verify: the test fails before 1.3.
- [x] 1.2 Test first: the digest request carries `chat_template_kwargs == {"thinking": false}`; the extraction request does not. Verify: fails before 1.3.
- [x] 1.3 Add the shared key (design 1.4) and use it in both builders (design 1.1, 1.2); also in the replay request (design 1.2); rename the audit test that asserts four keys. Verify: 1.1 and 1.2 pass, the replay test passes.

## 2. Completion timeout

- [x] 2.1 Test first, real short durations as the existing signed-call tests do (no paused time): (a) client: `chat_exact(body, limit)` against a wiremock delay of 1 s times out with `limit` 300 ms and succeeds with 2 s; (b) signed call with `Timing { deadline: 2 s, .. }`, a test reserve and a 1.2 s completion delay ends `signed`; (c) a delay past the deadline minus the reserve fails with the design 2.3 text and `state.json` stays `sending`; (d) when nothing is left after the reserve the call fails before sending and the state stays `preflight`. The reserve is a `Timing` field so tests can set it. Verify: fails before 2.2.
- [x] 2.2 Remove `CHAT_EXACT_TIMEOUT`, give `chat_exact` the `limit` parameter (move the client tests to it), compute the limit with saturating subtraction before writing `sending` (design 2.1 to 2.3). Verify: 2.1 passes.

## 3. Docs and gate

- [x] 3.1 `docs/privacy.md` per design 3. Verify: the review request shape and the bounds item name `chat_template_kwargs: {"thinking": false}` and the measured reason; no text says the request carries only four keys.
- [x] 3.2 Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate model-thinking-off --strict`.
