# Tasks

## 1. Script accounts

- [x] 1.1 Test first: `script_accounts` parse and refusals (unknown agent key, key not in `script_models`, unknown value); the statement wording for `anthropic-api`; a listing without the field keeps its statement and fingerprint (fixture). Verify: fail before 1.2.
- [x] 1.2 Implement design 3 (find the threading with `zg --rg -F "script_models"`). Verify: 1.1 passes.

## 2. Review notes

- [x] 2.1 Test first: notes rendered in order and escaped; more than 10 or longer than 300 refused; absent notes give the deliverable byte for byte as before. Verify: fail before 2.2.
- [x] 2.2 Implement design 4 in `crates/nearbee/src/audit.rs`. Verify: 2.1 passes.

## 3. Sandbox

- [x] 3.1 `sandbox/egress.allow` per design 1; update the tests that pin the statement's host list.
- [x] 3.2 `sandbox/Dockerfile` per design 2. Verify: `sandbox/build.sh` with `NEARBEE_BIN=$HOME/.local/share/nearbee/bin/nearbee` builds; inside `nearbee-sandbox:local` `go version`, `cargo --version`, `uv --version`, `osv-scanner --version`, `pip-audit --version`, `cc --version` print the pinned versions; run the bwrap probe and record it. Do not restart the running instance.
- [x] 3.3 Docs per design 5.
- [x] 3.4 Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate sandbox-toolchains --strict`.

## 4. Amendment: output files

- [x] 4.1 Test first: the `hire-serving` delta scenarios (files delivered and submitted; limits; resume without a second upload; secret in a file declines). Verify: fail before 4.2.
- [x] 4.2 Implement design 6 behind the `Market` seam. Verify: 4.1 passes; the old "output files not supported" scenario is replaced.
- [x] 4.3 Docs (`docs/market.md`, `docs/architecture.md`, `docs/privacy.md` where it says what the market keeps) and the native gate again (as 3.4).
