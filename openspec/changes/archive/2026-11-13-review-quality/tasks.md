# Tasks

## 1. Review and digest requests

- [x] 1.1 Test first: the review request has exactly the keys `model`, `stream`, `max_tokens`, `chat_template_kwargs`, `response_format`, `messages` with `response_format == {"type": "json_object"}`; the digest request carries the same `response_format`; the extraction request does not. Verify: fails before 1.3.
- [x] 1.2 Test first: a snapshot whose text passes 256 KiB packs below 256 KiB and omits the rest as `size_cap` (update the existing large-repository test). Verify: fails before 1.3.
- [x] 1.3 Implement design 1 and 2, the replay included. Verify: 1.1 and 1.2 pass.

## 2. Docs and gate

- [x] 2.1 Docs per design 4. Verify: `grep -rn "768 KiB" docs crates openspec/specs` prints nothing outside archived changes.
- [x] 2.2 Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate review-quality --strict`.
