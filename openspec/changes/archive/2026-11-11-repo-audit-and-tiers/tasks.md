# Tasks: repo audit and tiers

Implementer rules: `docs/process.md`, with these stage values and
exceptions. Change `repo-audit-and-tiers`, worktree
`~/projects/tools/nearbee-wt-s4`, report
`.reviews/repo-audit-and-tiers-implementer.md`; when the report exists,
read its "Next session handover" section first, continue from it and
append instead of rewriting.

**This implementer does not build.** Do not run `cargo` or any build, and
never put a build directory under `/tmp`, `/dev/shm` or the worktree.
Each session does the first group that still has open boxes not marked
**integrator**: write its tests and code, tick its boxes, write the
handover, stop. The integrator builds, runs the tests and clippy between
groups and writes failures into `.reviews/repo-audit-and-tiers-fixes.md`;
when that file has an open section, fix those items first and mark them
done there. Every group must leave the workspace compiling: when you
change a type or a trait, update every use and every fake in the same
group.

Before a group, read its decision in `design.md` and the spec it names.
apb routes and formats are read from apb source at `v1.0.0-beta.6`
(`git -C ~/projects/tools/agentic-playbooks show v1.0.0-beta.6:<path>`,
read only) and tested by replaying the recorded fixtures under
`crates/nearbee-apb/tests/fixtures/apb/`; NEAR AI Cloud and GitHub are
tested against local `wiremock` servers whose base URL the code takes as a
parameter (no test-only switches in production code). Every hire in this
stage is the owner's own test (D-023). No spend, no market write, no
`listing_status`.

## 0. Setup (integrator)

- [x] 0.1 **integrator** Add workspace dependencies `flate2 1.1.11`, `tar 0.4.46`, `getrandom 0.4.3`; create crate `crates/nearbee-cloud` (lib, depends on `nearbee-core`, `reqwest`, `serde_json`, `tokio`, `thiserror`); `cargo check --workspace` green.

## 1. Listing fields and materialisation (design 1; spec `listing-publish`, spec `privacy-tiers` Tiers)

- [x] 1.1 Tests in `crates/nearbee-core/src/listing.rs`: `tier` required with values `plain` and `attested` (scenarios "Tier missing", "Unknown tier"); `profile` validated; `intake_check: github_repo` accepted and any other value refused; an S3 `ResolvedListing` JSON (no new fields) still parses and is not serveable.
- [x] 1.2 Implement `Tier`, `IntakeCheck`, the new `Listing` and `ResolvedListing` fields; update every Rust initializer and every checked-in listing file (`playbooks/first-hire/listing.repo-audit-fast.yaml` gets `tier: plain`) and test fixture listing.
- [x] 1.3 Tests in `crates/nearbee-apb/src/install.rs`: with a profile, `defaults.profile` is replaced (created when absent); a node with its own `profile` key is refused naming the node; two profiles from one source give two different derived versions; without a profile the output is byte-identical to today's.
- [x] 1.4 Implement the `profile` argument of `materialise` (design 1.2) and pass `listing.profile` from `crates/nearbee/src/publish.rs`.

## 2. Catalog and the tier check (design 2, 3; spec `privacy-tiers`)

- [x] 2.1 `crates/nearbee-core/src/catalog.rs`: embed the catalog; tests that `deepseek-ai/DeepSeek-V4.1-Flash-1017` and `Qwen/Qwen3.8-27B` are owned by `nearai` and an `anthropic/` model is not.
- [x] 2.2 `crates/nearbee-core/src/tiers.rs`: the types of design 2, `check` and `evidence_models`; one test per scenario of spec `privacy-tiers` (review gate, interactive node, unknown kind, sub-playbook, scoped reference, unknown profile, incognito fallback, wrong route, OpenCode override as unknown routes, unknown model, skills, environment, env allowlist, connector, decisions on and unknown, compaction unknown), a passing attested and a passing plain case built like the shipped profiles, and `evidence_models` order and dedup. No trait change in this group.

## 3. Statement, fingerprint, drift (design 4.1 to 4.3; spec `data-statement`)

- [x] 3.1 Tests in `crates/nearbee-core/src/statement.rs`: plain on `nearbee-own-premium` names Claude and makes no TEE claim; plain with a connector and decisions on names them and says they may send data to providers Nearbee has not validated; attested names both TEE models in order and says the owner's machine can read the data; retention days and the not-covered list appear; byte-identical output for equal facts; fingerprint changes when two fallbacks swap and when tiers reorder; `drift` reports a missing field, a corrupt statement hash, an unsupported format, changed retention and a fingerprint mismatch, and `None` for a matching record.
- [x] 3.2 Implement `render`, `fingerprint`, `drift`, `STATEMENT_FORMAT`.
- [x] 3.3 **integrator** Record sanitized live responses from the instance on port 7322 (playbook detail, project and global profile detail, health with the apb version) into `crates/nearbee-apb/tests/fixtures/apb/` and note their shapes in `docs/apb.md`.

## 4. apb inspection, run params, attempts (design 2, 7, 8.2; spec `apb-runner`)

- [x] 4.1 Tests in `crates/nearbee-apb/tests/runner.rs` replaying the task 3.3 fixtures: `inspect` builds the facts with a temp workspace `opencode.json` and `.apb/config.yaml`; a scoped reference reads only its scope and `auto` falls back to global; a 404 profile is unknown with the status; a missing `opencode.json` or an `OPENCODE_` variable in the instance environment makes routes unknown; decisions false, true and absent; compaction follows the health version; `inspect_source` gives the same facts as `inspect` for the same YAML.
- [x] 4.2 Add `inspect` and `inspect_source` to `Runner`, implement them in `ApbRunner`, delegate them in `RoundRunner` (`crates/nearbee/src/lib.rs`), and implement them in every fake runner in the tests.
- [x] 4.3 Tests and implementation of `RunDetail.attempts` and `Reported` from `attempt_models` (fixture `run_succeeded.json`, plus a fixture with one reported model and one with mixed); update every `RunDetail` initializer.
- [x] 4.4 `crates/nearbee-apb/src/run_params.rs` `from_env` with a fixture `run.yaml` test (brief present, missing file, missing `APB_RUN_DIR`).

## 5. Publish (design 4.4; spec `listing-publish`)

- [x] 5.1 Tests in `crates/nearbee/tests/publish.rs`: an attested listing with a bad fallback fails before install and names the model; a check failing only after install leaves the old record and exits non-zero; differing source and installed fingerprints fail; a passing publish records tier, intake check, statement, hash, format, fingerprint and evidence models; the report prints `tier:` and the `## Data handling` block; still no market write.
- [x] 5.2 Implement the order of design 4.4 in `crates/nearbee/src/publish.rs`.

## 6. Serve: drift, intake check, statement snapshot, footer (design 4.5, 4.6, 5, 7, 8.1; specs `hire-serving`, `data-statement`, `repo-audit` Deterministic intake check)

- [x] 6.1 Generalise `waiting_decline` (design 4.6); existing serve tests stay green.
- [x] 6.2 Tests in `crates/nearbee-core/tests/serve.rs` for drift: an S3 record's new hire is declined `rejected` with `listing_drift` and no scope check, market start or run; decisions turned on before intake give the same and zero scope calls; a drift between rounds declines a waiting rework `failed`; a running hire of a drifted listing is still observed and delivered.
- [x] 6.3 Tests for the intake check: a brief with zero or two repositories is declined `rejected` with the spec's buyer text before any scope check or market start; one repository passes; listings without `intake_check` are unaffected. Tests for `intake::github_repos` per the scan scenarios.
- [x] 6.4 Tests for the statement snapshot: `statement.json` exists before the market start and before the `run_start` intent, and the intent carries `tier`, `statement_sha256`, `fingerprint`; a snapshot without an intent is reused when it matches and declined with `listing_drift` when the listing was relisted with other facts.
- [x] 6.5 Tests for delivery: the Markdown ends with `---` and the snapshotted statement; a footer that crosses the limit declines `failed`; a restart after the submission snapshot resends the same bytes; a legacy generation delivers with no footer; an S4 generation with a missing `statement.json` declines `failed` with `statement_missing`; the terminal `run_observed` record carries the attempts.
- [x] 6.6 Implement the drift pass, the intake check, the snapshot, the assembly, the legacy rule and the attempts in `run_observed`.

## 7. Endpoint evidence (design 6; spec `run-evidence`)

- [x] 7.1 Tests in `crates/nearbee-cloud` against `wiremock`: path, query (`model` encoded, `signing_algo=ecdsa`, 64-hex `nonce`) and bearer header; 301 is a failure and not followed; a 3 MiB body fails at the cap; 503 then 200 succeeds; two 503 fail; the whole fetch stays within 20 s; the key never appears in an error text.
- [x] 7.2 Implement `CloudAttestor` behind `nearbee_core::evidence::Attestor`.
- [x] 7.3 Tests for the journal events and snapshot records (round trip, flat names, hash checked on reuse).
- [x] 7.4 Tests in `crates/nearbee-core/tests/serve.rs` with a fake attestor: start reports collected one per round before the market start and the run; a hire waiting for evidence makes no market start; the third failed start fetch declines `rejected` (unstarted first generation) or `failed` (owner-started, rework, redo) with `evidence_unavailable`; a file without its record is completed without a fetch; a missing start report after the `run_start` intent declines `failed` with `evidence_missing`; end reports collected after a succeeded run; the third failed end fetch declines `failed`; plain listings never call the attestor; another hire's near deadline blocks the fetch in that round.
- [x] 7.5 Tests for `evidence::section`: per model the id, both hashes and times, the link with `%2F`; per attempt requested (or `not recorded`) and reported, `none reported` or `mixed`; both wording sentences.
- [x] 7.6 Implement the scheduling, start and end steps, the renderer, and the wiring in `nearbee serve` (`crates/nearbee/src/lib.rs`, key from `Config.nearai_api_key`).

## 8. Agent A tools (design 8; spec `repo-audit`)

- [x] 8.1 Tests for the archive reader with tarballs built in the test (`tar::Builder`): files land under `audit-snapshot/repo/` without the top directory; symlink, hard link, `..` and absolute members are skipped and counted; a 2 MiB file and a binary file are skipped; skipped data counts against the decompressed cap; the member cap fails; a truncated gzip fails; a duplicate path and a rename collision fail; instruction files are renamed; a failed run leaves no `audit-snapshot/`; a leftover staging directory, also a link, is removed and not followed.
- [x] 8.2 Tests for `fetch_snapshot` against `wiremock` (both bases are parameters): private repository refused; 301 refused; the SHA lands in `source.json`; a published snapshot for the same repository is reused with no request; a brief with two repositories fails.
- [x] 8.3 Tests for `finish`: a grounded finding is accepted; unknown file, line out of range, empty excerpt and excerpt not contiguous are rejected with reasons; `version: 2`, an unknown field, invalid JSON, a linked or oversized `candidates.json` fail with no `out/result.json`; a failed finish after a successful one leaves no `out/result.json`; escaping and fence length (including JSON with no backticks: three); `out/result.json` validates with `nearbee_core::manifest::validate`.
- [x] 8.4 Implement `fetch_snapshot`, `finish` and the `audit fetch` and `audit finish` subcommands (async, design 8.3).

## 9. Playbook, profiles, listings (design 9; specs `executor-profiles`, `repo-audit`)

- [x] 9.1 `playbooks/profiles/nearbee-near.yaml` DeepSeek first, Qwen fallback; new `playbooks/profiles/nearbee-own-premium.yaml`; `crates/nearbee/tests/profiles.rs` checks the three names, the model order and the premium template.
- [x] 9.2 `playbooks/repo-audit/` with `playbook.yaml`, `scripts/fetch.sh`, `scripts/finish.sh` and the two listing files of design 9; the agent prompt spells out the candidate schema and the rules of spec `repo-audit`, Candidate findings; a test loads both listing files and materialises both variants.
- [x] 9.3 Delete `playbooks/first-hire/`; move every test, doc and script reference to `playbooks/repo-audit/`.
- [x] 9.4 `scripts/apb-instance.sh install`: install `nearbee-own-premium`, write `decisions: { enabled: false }` into the project `.apb/config.yaml`, set `NEARBEE_BIN`, set no `OPENCODE_` variable; extend `crates/nearbee/tests/apb_instance_script.rs`.

## 10. Docs

- [x] 10.1 `docs/privacy.md`: the tiers as built (checks, what is not proved, OpenCode overrides, decisions off and the skipped scope check), the statement, the evidence as collected-not-verified, the signed-step contract of design 10 under "Attested, signed" marked for S5.
- [x] 10.2 `docs/showcase-agents.md` agent A as built (GitHub snapshot, intake check, embedded JSON, no "never left a TEE" claim); `docs/stages.md` S4 and S5 (signed step moves to S5; S4 done-when as in group 11); `docs/owner-checklist.md` O-11 reworded (no apb profile setting confines file access; the statement says so); `docs/architecture.md` module map (`tiers`, `statement`, `evidence`, `catalog`, `run_params`, `nearbee-cloud`, `audit`); `playbooks/README.md`; `docs/near-ai-cloud.md` (attestation needs the key, Verification Center URL form).

## 11. Gate and live (integrator)

- [x] 11.1 **integrator** Full native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate repo-audit-and-tiers --strict`.
- [x] 11.2 **integrator** `scripts/apb-instance.sh install`, then `nearbee list playbooks/repo-audit/listing.repo-audit-fast.yaml`; its record carries tier and fingerprint; the attested check passes on the live instance.
- [ ] 11.3 **integrator** `nearbee list` for the premium listing once the owner has created its draft agent and token (O-2); until then report it as waiting on O-2.
- [x] 11.4 **integrator** Live run of the premium variant's playbook through apb on a real public repository (the owner's subscription, no Cloud spend): `out/findings.json`, `out/deliverable.md` and a valid manifest; record it in `docs/demo-log.md`.
- [x] 11.5 **integrator** Live attestation report fetch for both TEE models (not billable); record a sanitized response as a replay fixture for `crates/nearbee-cloud` and the result in `docs/near-ai-cloud.md`; if it fails, report the gap.
- [ ] 11.6 **owner** The fast variant's live run (Cloud credits, O-3) and setting both listings live (O-6): reported, not done by an agent.

## Workflow follow-up

- Quality cycle of `docs/process.md` (one review round, owner 2026-11-11), archive, pull request.
