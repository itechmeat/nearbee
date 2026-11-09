# Tasks: foundation and runner

Implementer rules: `docs/process.md`. Stage values: change
`foundation-and-runner`, so `CARGO_TARGET_DIR=$HOME/.cache/nearbee-foundation-and-runner-target`.
Before a group, read its part of `design.md` and the spec it names. No buyer
data exists in this stage: synthetic text only.

## 1. Workspace and CI

- [x] 1.1 Create the cargo workspace of `design.md` 1 with the four crates, `rust-toolchain.toml`, `[workspace.dependencies]` at the newest stable versions (check each on crates.io before adding; record the versions in the task report), `unsafe_code = "forbid"`; verify `cargo check --workspace` passes.
- [x] 1.2 Add `.github/workflows/ci.yml` running `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `openspec validate --all --strict` (installing the OpenSpec CLI with npm); verify the workflow file parses (`actionlint` if installed, else a YAML parse) and the four commands pass locally.

## 2. Config (spec `nearbee-config`)

- [x] 2.1 `Secret` type with redacted `Debug`/`Display`; test that formatting never shows the value.
- [x] 2.2 Layered loading (defaults, YAML via `yaml_serde` with unknown keys refused, `.env` via `dotenvy` without overriding the process, process environment; relative paths against the file's directory); tests for every scenario of the spec using `tempfile`.
- [x] 2.3 `nearbee.example.yaml` and `.env.example` (placeholders only); test that the example file loads.

## 3. Journal types (spec `hire-journal`)

- [x] 3.1 `safe_id`, `Record` envelope, event enum with `Unknown`, `Intent`/`Result` with `operation_id`; tests for round trip, unknown event type, newer schema version refused, unsafe id characters and `a/b` vs `a_b` staying distinct.
- [x] 3.2 Pure reader: lines to records, open intents, truncated trailing line reported, malformed interior line an error; tests for each case.
- [x] 3.3 `Received` payload holds brief SHA-256 and length only; test that a brief's text is absent from the serialized record.

## 4. Manifest (spec `result-manifest`)

- [x] 4.1 Schema with `deny_unknown_fields`; tests for minimal valid, unknown version, unknown field, duplicate path, Markdown in a subdirectory.
- [x] 4.2 Containment walk of `design.md` 5; tests with real temp dirs for traversal, absolute path, symlinked `out`, symlinked entry, symlinked parent directory, non-regular file (a FIFO on unix).
- [x] 4.3 Bounded read, size and SHA-256 check, validated bytes kept; tests for hash mismatch, size mismatch, file over the limit (read stops at limit + 1), too many files, total over the limit.
- [x] 4.4 Markdown UTF-8 and 262,144-byte limit plus `check_final_markdown`; tests for 262,144 ok, 262,145 refused, invalid UTF-8, the footer case.
- [x] 4.5 Missing manifest is an error; test.

## 5. apb runner (spec `apb-runner`)

- [x] 5.1 Read `docs/apb.md` and the apb docs `DEPLOYMENT.md`, `HOWTO-authoring.md` (params, `search: true`), and find how a directory becomes an apb project; write `scripts/apb-instance.sh` of `design.md` 10 and start Nearbee's own apb instance on port 7322 (never touch the owner's dashboard on 7321 or `~/.config/apb`); create the dedicated workspace directory (default `~/.local/share/nearbee/apb-workspace`, a git repository) and a second throwaway project, register both in that instance only; record the steps in `docs/apb.md`. Verify `curl -s http://127.0.0.1:7322/api/projects` lists both.
- [x] 5.2 Write `playbooks/profiles/nearbee-own.yaml` (the owner's subscription executor, used for runs that spend no Cloud credit) and the smoke playbook of `design.md` 8 as `smoke-qwen` and `smoke-deepseek` in the dedicated workspace (sources kept in `playbooks/smoke/`), each with `topic`, `hire_id` and `execution_id` params (the last two `search: true`); verify `apb validate` passes on both.
- [x] 5.3 `capture` example binary and fixtures for: projects, playbooks, estimate, start (a run of the smoke playbook on the `nearbee-own` profile, no Cloud spend), run detail while running and after `succeeded`, search by `hire_id` and by `execution_id`, a 409 or 422 refusal, a 404 run, and a playbook list that includes an entry of a second workspace (any other registered apb project); capture the running state right after the start, and if the run finished first, note the skipped capture in the report instead of retrying; verify fixtures contain no key and no home path.
- [x] 5.4 `ApbRunner` with the client of `design.md` 3 and workspace pinning; wiremock tests replaying each fixture, matching method, path, query and body, including the second-workspace playbook being ignored, the reserved-param refusal and the queued 200 start.
- [x] 5.5 Error classes and redaction; tests: timeout on start gives start-outcome-unknown with no second request, plain-text 422 gives refused, 409 `budget_needs_decision` keeps code and estimate, retryable read retried 3 times then fails, bearer key absent from every error and log line.
- [x] 5.6 Status mapping and usage; tests for every status of the spec, an unknown status, missing cost reported as unknown.
- [x] 5.7 Lookup with completeness; tests for complete, withheld, not-indexed.
- [x] 5.8 Update `docs/apb.md` with what the fixtures showed: auth (the header is a CSRF marker, not a credential), run detail fields and statuses, workspace selection on every call, the worktree rule, `on_no_estimate: continue` (remove "pause once estimates exist"), `execution_id`; verify each statement against a fixture.

## 6. CLI

- [x] 6.1 `nearbee config check` per the spec, with tests for all-present, dashboard down, missing workspace (wiremock and an unused port).
- [x] 6.2 `nearbee runs find` per the spec requirement "Run find command" (output and the exit codes 0, 1, 2); tests against wiremock for each scenario; verify once against the live dashboard with the run of 5.3.

## 7. Profiles and the Cloud path (spec `executor-profiles`)

- [x] 7.1 `playbooks/profiles/nearbee-near.yaml` per `design.md` 8, plus `nearbee-near-vision.md` marking the vision template unsupported with the reason; a test parses the near profile and checks every model id is `nearai`-owned in `docs/reference/near-ai-cloud-models.json` and that no value looks like a key.
- [x] 7.2 OpenCode provider config in the dedicated workspace; Nearbee's apb instance carrying `NEARAI_API_KEY` from `.env` (document how in `docs/apb.md`, values never printed); verify with `nearbee config check` and `apb detect` that the provider and models are seen.
- 7.3 Moved to stage S2 by the owner on 2026-11-09 (the NEAR AI Cloud balance is empty; the live runs are done once it is topped up): see `docs/stages.md` S2, item 0.

## 8. Docs and gate

- [x] 8.1 `docs/architecture.md`: core may run narrow synchronous file checks (manifest); an empty lookup after an ambiguous start does not prove no run exists; `execution_id` beside `hire_id`; profile names `nearbee-own`, `nearbee-near`; the `apb:` block of the config example replaced with the S1 shape (`url` on port 7322, `workspace_path`). `README.md` "Try it": the S1 commands (`config check`, `runs find`). `docs/open-questions.md`: mark item 12 answers found, keep the rest.
- [x] 8.2 Full native gate green: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate foundation-and-runner --strict`; paste the last lines of each into the report.

## Workflow follow-up

- The integrator runs the quality cycle of `docs/process.md`, archives the change with `openspec archive foundation-and-runner --yes`, and opens the pull request.
- After the merge: remove the worktree and `~/.cache/nearbee-foundation-and-runner-target` (`cargo sweep` on the main target dir if any).
