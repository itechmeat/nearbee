# Proposal

## Why

Stage S6 (`docs/stages.md`, plan items 18, 19 and 20) gets Nearbee ready
for the judges and the demo video: a judge on a clean machine must see the
hire machine work without keys, apb or a market account, and the live demo
must make only claims the code enforces. Two facts of 2026-11-11 change
the stage: apb `1.0.0-beta.7` ships the external-data run flag D-023 waits
for (and, as installed, makes the S4 tier check refuse every listing,
because compaction is known off only for beta.6), and OpenCode 2.2.5 runs
agent A's attested model step inside a shared background service with the
owner's configuration and MCP servers, so the attested promise does not
hold.

## What Changes

1. `nearbee replay`: a credential-free walkthrough of the real hire state
   machine (`serve::round`) against scripted in-process market, runner and
   attestor implementations: success, oversized decline, redo, restart,
   cancellation; exit 0 only when every scenario ends as expected; CI runs
   it.
2. Journal records take their time from the injected `Clock`, so a replay
   (and any test) drives deadlines and intent staleness deterministically.
3. apb beta.7: the instance version is parsed as semver; compaction is
   known off for `1.0.0-beta.6` and `1.0.0-beta.7`; serve refuses to start
   a run on an apb older than `1.0.0-beta.7`, sends `external_data: true`
   on every market run and checks the flag on every observed run detail; a
   missing flag aborts the run and declines the hire `failed`;
   `nearbee config check` prints the apb version. D-023's "owner's own
   test hires only" rule is lifted.
4. The attested tier refuses the `opencode` agent (routes unknown, the
   shared service named). Agent A's fast listing moves to its own playbook
   `playbooks/repo-audit/playbook.fast.yaml`: `fetch`, then a new script
   step `nearbee audit review` (packs the snapshot, one signed DeepSeek
   V4.1 Flash call through the S5 signed-call code, writes
   `audit/candidates.json`), then the existing `finish`. The listing gains
   `direct_models` and `signed_models`; its deliverable carries the
   Verification Center receipt. The premium listing keeps the agent
   playbook on `claude`.
5. README: a replay-first "Try it", apb beta.7, the status line and the
   live steps corrected; the integrator follows it on a clean clone.
6. Docs: `docs/apb.md` (beta.7 facts, the OpenCode service finding),
   `docs/privacy.md`, `docs/decisions.md` (D-023 status, D-027),
   `docs/demo-script.md` (shot list), `docs/x-post.md` (draft text),
   `docs/submission.md` (payload draft, owner fields open).

Out of scope: `nearbee login` (Deferred, `docs/stages.md`); a prebuilt
release binary; any apb change (apb is the owner's repository); spending,
publishing listings, posting on X and sending the submission (owner only,
`docs/hackathon.md`).

## Capabilities

### New Capabilities

- `replay-walkthrough`: the `nearbee replay` command, its scenarios, its
  isolation from credentials, network and apb, and its exit codes.

### Modified Capabilities

- `apb-runner`: run start carries `external_data`; run observation reads
  it; the instance version is read and compared.
- `hire-serving`: version gate before a start, the external-data flag on
  every market run and its check on every observed run detail.
- `hire-journal`: record timestamps come from the serve clock.
- `privacy-tiers`: compaction known for beta.6 and beta.7; the attested
  tier refuses `opencode`; direct models cover Nearbee's audit command.
- `repo-audit`: the fast variant's review step and its packing, request
  and output rules.
- `nearbee-config`: the config check reports the apb version.

## Impact

1. Crates: `nearbee-core` (`serve.rs`, `journal.rs`, `runner.rs`,
   `tiers.rs`), `nearbee-apb` (`lib.rs` start body and run detail,
   `inspect.rs` version and compaction), `nearbee` (`replay` module,
   `audit.rs` review, `main.rs`). The seam rule holds: the replay's market,
   runner and attestor implement the core traits and live in the `nearbee`
   crate; the core imports no implementation.
2. Dependencies: `semver` (newest stable, checked before adding).
3. Playbooks: `playbooks/repo-audit/playbook.fast.yaml`, its script
   `scripts/review.sh`, the fast listing file.
4. CI: one more step, `cargo run -p nearbee -- replay`.
5. Live (integrator, owner-gated where it spends): relisting both example
   listings after the merge (no market write), the first live fast run and
   the paid hire need O-3 credits and O-5.
