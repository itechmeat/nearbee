# Tasks

## 1. Listing

- [x] 1.1 Test first: the `listing-publish` delta scenarios (kept entry, malformed, duplicate) in `crates/nearbee-core/src/listing.rs` tests. Verify: fail before 1.2.
- [x] 1.2 Implement design 1 (parse, check, `ResolvedListing.script_models`, recorded and frozen like `direct_models`). Verify: 1.1 passes.

## 2. Tier rule and facts

- [x] 2.1 Test first: attested with `script_models` refused naming `script_models` and tier plain; plain accepted; facts without the field fingerprint exactly as on main (fixture). Verify: fail before 2.2.
- [x] 2.2 Implement design 2 and 3 (`ExecutionFacts.script_models`, `with_script_models`, drift pass). Verify: 2.1 passes.

## 3. Statement

- [x] 3.1 Test first: the `data-statement` delta scenarios (ux-flow wording, unchanged without the field). Verify: fail before 3.2.
- [x] 3.2 Implement design 4. Verify: 3.1 passes.

## 4. Run param

- [x] 4.1 Test first: the `apb-runner` delta scenarios via `sent_params`, a playbook param named `script_models` refused, and the list threaded from the listing record into a start request in serve and replay. Verify: fail before 4.2.
- [x] 4.2 Implement design 5 everywhere `direct_models` is threaded (find with `zg --rg -F "direct_models"`). Verify: 4.1 passes.

## 5. Sandbox and docs

- [x] 5.1 `sandbox/Dockerfile` per design 6. Verify: `sandbox/build.sh` with `NEARBEE_BIN=$HOME/.local/share/nearbee/bin/nearbee` builds, and `docker run --rm nearbee-sandbox:local claude --version` prints 2.1.319. Do not restart the running instance.
- [x] 5.2 `docs/privacy.md` (listing field, statement wording quoted, design 7 rule), `docs/apb.md` (`NEARBEE_APB_ENV_VARS` example with `CLAUDE_CODE_OAUTH_TOKEN`, the CLI in the image), and any doc that lists listing fields (`zg "listing fields direct_models"`). Verify: wording matches design 4 and 7.
- [x] 5.3 Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate script-models --strict`.

## 6. Amendment: any agent CLI, DeepSeek Harness

- [x] 6.1 Test first: the new `listing-publish` scenario (any agent CLI, dsh entry kept) and a bad agent name (`Claude/x`, `cl aude/x`) refused; the `data-statement` dsh scenario. Verify: fail before 6.2.
- [x] 6.2 Implement the amended design 1 and 4 (open agent names; dsh wording; generic line for other agents). Verify: 6.1 passes, earlier tests unchanged.
- [x] 6.3 Design 6a: `sandbox/Dockerfile` npm `@deepseek-ai/dsh@0.2.2-rc.3`, `sandbox/egress.allow` `api.z.ai`; docs (`docs/apb.md` `NEARBEE_APB_ENV_VARS` with `ZAI_API_KEY`, `docs/privacy.md` the dsh wording). Verify: image builds; `docker run --rm nearbee-sandbox:local dsh --version` prints 0.2.2-rc.3. Do not restart the running instance.
- [x] 6.4 Native gate again (as 5.3).
