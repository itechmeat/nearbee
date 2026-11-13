# Design: review quality

1. `audit::REVIEW_REQUEST_CAP = 256 * 1024`. Packing logic unchanged.
2. A shared `nearbee_cloud::json_object()` returning `{"type": "json_object"}`,
   added as `response_format` to `audit::review_body`, the digest request in
   `digest.rs` and the replay's review-shaped request in `replay/runner.rs`.
   The vision extraction request does not change (plain text answer).
3. `signed_call::check_request` already allows extra keys; the key is inside
   the signed request bytes.
4. Docs: the request shape in `docs/privacy.md` and the cap wherever 768 KiB
   is named (`grep -rn "768 KiB" docs openspec/specs crates`).
5. Live check (integrator): the paid fast audit of `itechmeat/nearbee`
   again, findings expected; then a declined hire with `serve --reject`.
