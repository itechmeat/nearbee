# Design: model thinking off

## 1. Request key

1.1. `audit::review_body` adds `"chat_template_kwargs": {"thinking": false}`
to the JSON object it already builds (model, stream, max_tokens, messages).
The doc comment changes from "nothing else" to name the five keys.

1.2. The digest request in `digest.rs` (the `DIGEST_MODEL` body) and the
replay's review-shaped request (`replay/runner.rs`) add the same key, so the
replay's evidence matches the real request shape. The vision extraction body (`extract_body`, Qwen instruct model)
does not change.

1.3. `signed_call::check_request` does not restrict extra keys today and is
not changed: the key is sent inside the exact request bytes, so the
signature's `request_sha256` covers it.

1.4. The key is a constant in one place (`nearbee-cloud`, for example
`pub fn thinking_off() -> serde_json::Value`), so both builders share it.

## 2. Completion timeout

2.1. `CHAT_EXACT_TIMEOUT` (240 s) is removed. `Client::chat_exact(&self,
body: &[u8], limit: Duration)` waits for `limit` alone; `signed_call` is its
only caller, and the existing client tests move to the new signature.

2.2. `signed_call` keeps a signature reserve, `SIGNATURE_RESERVE` = 30 s
(at least the signature fetch timeout plus a few signature intervals), so a
completion that lands late still leaves time to fetch its signature. Before
it writes the `sending` state it computes
`limit = timing.deadline.saturating_sub(started.elapsed()).saturating_sub(SIGNATURE_RESERVE)`;
when `limit` is zero it fails with the bound-reached error while the state
is still `preflight`, so nothing is marked as sent. Otherwise it passes
`limit` to `chat_exact`. One invocation stays bounded by 360 s.

2.3. The timeout error names the bound, not a computed remainder:
"the completion did not answer within the invocation's N s bound; not
resent", where N is `timing.deadline` in seconds. Tests assert this text.

2.4. External call: `POST /v1/chat/completions`, no retry (unchanged); a
resumed invocation decides from the state file whether to resend
(unchanged).

## 3. Docs

`docs/privacy.md` (the review request shape near line 239 and the bounds
list near line 358) names the new key and says why: the model's reasoning
otherwise outruns the bound (measured 2026-11-12).

## 4. Live check (integrator, after merge)

One fast audit of a small public repository in the sandbox and one on the
host instance, inside the owner's USD 1 test budget. The check passes only
when `signature.json` is written (the signed text matches the request and
response hashes) and the deliverable is produced; the result goes into
`docs/demo-log.md`.
