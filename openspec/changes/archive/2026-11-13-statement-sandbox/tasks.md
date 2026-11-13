# Tasks

## 1. Fact

- [x] 1.1 Test first: the argv parser yields the sorted hosts for the measured sandbox argv, `None` when any required flag is missing, when the image is not `nearbee-sandbox:*`, or when the command is not `docker run`. Verify: fails before 1.3.
- [x] 1.2 Test first: facts without a sandbox serialize byte for byte as before (compare with a fixture fingerprint from main). Verify: fails only if 1.3 changes it.
- [x] 1.3 Implement design 1 and 2. Verify: 1.1 and 1.2 pass; `cargo test -p nearbee-apb -p nearbee-core` passes.

## 2. Statement

- [x] 2.1 Test first: the scenarios of the `data-statement` delta (sandboxed, no fact unchanged, attested). Verify: fails before 2.2.
- [x] 2.2 Implement design 3. Verify: 2.1 passes.

## 3. Docs and gate

- [x] 3.1 `docs/privacy.md` per design 5. Verify: both variants quoted.
- [x] 3.2 Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate statement-sandbox --strict`. Search the code with `zg` (AGENTS.md rule 11).
