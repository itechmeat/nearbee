# Tasks: judges path and submission

Implementer rules: `docs/process.md`. Change `judges-path-and-submission`,
worktree `~/projects/tools/nearbee-wt-s6` (branch
`s6-judges-path-and-submission` off a fresh `main`),
`CARGO_TARGET_DIR=$HOME/.cache/nearbee-judges-path-target`, report
`.reviews/judges-path-and-submission-implementer.md` (when it exists, read
its "Next session handover" first and append). Before a group, read its
decision in `design.md` and the spec it names; `brainstorm.md` holds the
evidence. No billed call, no market write, no spend: NEAR AI Cloud is
tested against `wiremock` and recorded fixtures only. Never touch the
owner's apb (port 7321), `~/.config/apb`, the apb repository, or the
owner's OpenCode service. Tasks marked **integrator** or **owner** are not
yours.

## 1. Journal clock (design 2; spec `hire-journal`)

- [x] 1.1 Tests first: a writer opened with a fixed clock stamps that time; a redo deadline is reached when the injected clock passes it (in `crates/nearbee-core/tests/serve.rs`, using the existing driver); they fail before the code exists.
- [x] 1.2 Move `Clock` to `nearbee_core::clock` (re-export from `serve`), give `journal::Writer` a clock, pass the round clock from serve and `Clock::real()` elsewhere; `cargo test -p nearbee-core` passes.

## 2. apb beta.7 and external-data runs (design 3; specs `apb-runner`, `hire-serving`, `nearbee-config`, `privacy-tiers` Compaction by apb version)

- [x] 2.1 Record live fixtures from Nearbee's own instance (port 7322, check `GET /api/health` says `1.0.0-beta.7` first): the health answer as `crates/nearbee-apb/tests/fixtures/apb/health_beta7_live.json`, and a real flagged run: install a throwaway script-only playbook (one `script` node that writes nothing outside its worktree, no agent, no model) into Nearbee's workspace, start it once over REST with `external_data: true`, record its `GET /api/runs/{id}` detail (home paths replaced by `/home/user`), then delete that playbook from the workspace files. No other run, no agent call.
- [x] 2.2 Tests first: `external_data_supported` for `1.0.0-beta.6` (false), `1.0.0-beta.7`, `1.0.0-beta.10`, `1.0.0` (true) and `dev` (protocol error); start body with and without the flag; detail with and without the flag; compaction known off for beta.6 and beta.7 and unknown for beta.8; serve: an unsupported apb at intake declines `rejected` before the market start with `apb_too_old`, an error at intake writes nothing and retries, an unsupported apb before a rework starts nothing, every run start carries the flag (first run, rework, redo), a detail without the flag on the first run and on a rework aborts, journals `privacy_flag_missing`, declines `failed` with "the run could not be isolated" and raises `privacy_incident`; config check ok and failed lines.
- [x] 2.3 Implement: `semver` in `nearbee-apb` only (check the newest stable version first), `Runner::external_data_supported` in every implementation (apb runner, the wrappers in `crates/nearbee/src/lib.rs`, the test fakes), `StartRequest.external_data`, `RunDetail.external_data`, the two gates and the check in serve, the compaction set, the config check line; the live smoke test and `examples/capture.rs` send no flag. `cargo test --workspace` passes.

## 3. Attested tier without agents (design 4; spec `privacy-tiers` Attested model route)

- [x] 3.1 Tests first: an attested listing whose profile runs `opencode` on a `nearai/` model is refused with the shared-service breach; a `claude` fallback is refused; an `agent_task` or `prompt` node with no profile and no default profile is refused naming the node; a script-only attested playbook with `nearai` direct models passes; the S4 attested OpenCode scenarios now end refused by the agent rule; the S4 OpenCode fact checks keep their tests.
- [x] 3.2 Implement in `crates/nearbee-core/src/tiers.rs`; update the statement text if it names the OpenCode route for attested listings.

## 4. Agent A fast on a signed review (design 5; spec `repo-audit`)

- [x] 4.1 Tests first for `nearbee audit review`: deterministic pack (same snapshot, same bytes), priority order, 768 KiB cap at a file boundary with the omitted list, non-UTF-8 file omitted, the request has only `model`, `messages`, `stream: false` and `max_tokens: 16384`, a model outside `direct_models` fails before any request, the signed call against `wiremock` with the S5 Cloud fixtures, a rework makes a new call, a fenced JSON answer is unwrapped, an unparsable answer or one with 41 findings fails the step with `review_unparsed`; `finish` (execution id from the run params) copies the four evidence files and lists them with `digest::EVIDENCE` kinds when the signature exists and adds nothing otherwise, lists the omitted files in the coverage section, and serve's signature recheck accepts the fast manifest.
- [x] 4.2 Implement `audit review` in `crates/nearbee/src/audit.rs` (reuse `signed_call::signed_call` and the digest command's evidence pattern), `playbooks/repo-audit/playbook.fast.yaml` (no `defaults.profile`), `scripts/review.sh`, and the fast listing file (`source_playbook`, no `profile`, `direct_models`, `signed_models`); keep the premium listing and `playbook.yaml` as they are; make the Response signature section's words generic (design 5.7).
- [ ] 4.4 Fallback (design 5.8): if this group is not accepted by 2026-11-13 12:00 UTC, stop it, leave the fast listing file as it is on `main` and say so in the report; the integrator then updates the demo script.
- [x] 4.3 Add a test that loads both repo-audit listing files and the fast playbook through the listing and tier code with recorded facts and expects the fast one to pass as attested and the premium one as plain.

## 5. Replay (design 1; spec `replay-walkthrough`)

- [x] 5.1 Tests first (`crates/nearbee/tests/replay.rs`, running the built binary): all five scenarios pass and exit 0; `--scenario restart` runs one and its output shows the start intent resolved by lookup; an unknown name exits 2; a non-empty `--keep` directory exits 2; `--verbose` adds journal event names; with `HOME` and `XDG_CONFIG_HOME` pointed at an empty directory and `PATH` without apb it still passes; the output has no uuid (regex check) and starts with the banner, which says the signature is synthetic.
- [x] 5.2 Implement `crates/nearbee/src/replay/` and the subcommand; fixtures embedded with `include_str!`; no network, no process, no config, no credentials.
- [x] 5.3 CI: add `cargo run -p nearbee --locked -- replay` after the tests in `.github/workflows/`.

## 6. Docs (design 3.8, 4, 5, 6)

- [x] 6.1 README: "Try it" opens with the replay (command, expected last lines, cold build time you measured), then the live steps renumbered with apb `1.0.0-beta.7` and the fast listing's new shape; status line for S6; no marketing.
- [x] 6.2 `docs/apb.md`: beta.7 facts (external data on start and detail, health version, compaction re-read with the evidence of `brainstorm.md` approach 5, the native `nearai/` route on `dsh` and why Nearbee does not use it); `docs/privacy.md` and the plain statement text: external-data runs, the incident path, earlier unflagged runs, what the flag does not reach (Claude Code keeps sessions under the owner's home for the premium listing), the attested tier without agents, the OpenCode service finding; `docs/decisions.md`: D-023 status, new D-027 (attested listings use Nearbee's direct calls only; agent A fast is a signed review); `docs/architecture.md` (replay, audit review), `docs/stages.md` S6 status, `docs/near-ai-cloud.md` if a measured fact changed.
- [x] 6.3 `docs/demo-script.md` per design 6.2.

## 7. Integrator and owner

- [x] 7.1 **integrator** Acceptance against every requirement and scenario; rerun the gates.
- [x] 7.2 **integrator** Clean-clone run: clone the branch into `~/.cache/nearbee-clean-clone`, follow README step 1 exactly, record the time and the output tail in the report, delete the clone.
- [x] 7.3 **integrator** Quality cycle (one review round, owner 2026-11-11), report `docs/reviews/<date>-judges-path-and-submission.md`, archive, pull request, merge.
- [x] 7.4 **integrator** `docs/x-post.md` and `docs/submission.md` drafts (design 6.3, 6.4).
- [ ] 7.5 **integrator** After the merge: rebuild `nearbee` into `NEARBEE_BIN`, restart `nearbee-apb`, `nearbee list` the fast listing (no market write; the only one with a token) and check it passes as attested; the other two when O-2 is done.
- [ ] 7.6 **owner** O-3 credits, O-2 tokens, O-5 caller account, O-6 go-ahead; then the live fast run, agent B on an invoice scan, the paid x402 hire, each recorded in `docs/demo-log.md`; then O-4, O-8, O-9.
