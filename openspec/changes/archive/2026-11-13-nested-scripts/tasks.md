# Tasks

- [x] 1. Test first: the three scenarios of the delta (nested order and flags, flat names and derived version unchanged against a fixture, symlink refused naming the path). Verify: they fail before task 2.
- [x] 2. Implement design 1 and 2. Verify: the new tests and `cargo test -p nearbee` pass.
- [x] 3. Docs per design 4. Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate nested-scripts --strict`. Search the code with `zg`.
