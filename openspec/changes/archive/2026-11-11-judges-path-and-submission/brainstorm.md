# Brainstorm

## Problem

S6 makes everything code-side ready for the owner to record the demo video
and send the submission: a judge on a clean machine must see Nearbee work
without keys, apb or a market account, and the live path the video shows
must be honest about what it proves. Two facts arrived on 2026-11-11 that
reshape the stage: apb 1.0.0-beta.7 shipped the external-data run flag
(D-023 can be lifted) and the native NEAR route, and an OpenCode probe on
this machine showed the attested agent-A route leaks the owner's
environment. The implementer is a Claude Opus coding agent; about one day.

## Related work

1. `docs/stages.md` S6, plan items 18 to 20; `docs/hackathon.md` (judging
   signals, submission fields).
2. Decisions D-012, D-023 (external data), D-026 (direct models, signed
   calls).
3. S4 (`repo-audit-and-tiers`): the tier check, `inspect.rs`, agent A on
   OpenCode. S5 (`doc-digest-and-paid-demo`): `signed-call`, direct and
   signed models, agent B on script nodes only.
4. apb `v1.0.0-beta.7` (`docs/EXTERNAL-DATA.md`, `docs/AGENT-SETUP.md`
   "DeepSeek Harness", `docs/PROFILES.md` "Agent environment").

## Doubts

1. What does `replay` replay, and at which layer, given only four live
   captures exist and none records a start, submit, decline or redo answer?
2. Can the replay be deterministic when journal records carry the real time
   and deadlines are read from them?
3. What does serve do when a run detail lacks `external_data` after the
   start (buyer data already reached apb), and how is a prerelease apb
   version compared?
4. Does apb beta.7 break the S4 tier check?
5. Is OpenCode, as installed (2.2.5), a route the attested tier can vouch
   for?
6. What does a judge need on a clean machine?

## Approaches considered

1. Replay through the real HTTP client against an in-process server serving
   captures (author). Evidence: `crates/nearbee-market/tests/fixtures/market/`
   holds agent, assignments, file-trio and pricing-plans only; every write
   answer would be invented, and the client is already covered by
   `tests/replay.rs` and `contract.rs`. Cost high, value low.
2. Replay at `serve::round` (`crates/nearbee-core/src/serve.rs`) with small
   scripted `Market`, `Runner` and `Attestor` implementations shipped in
   the `nearbee` crate, rows shaped like the captures, transitions scripted
   per round (partner). The test fakes in `crates/nearbee-core/tests/serve.rs`
   stay where they are; moving 8.6k lines is the trap.
3. Journal timestamps from the injected `Clock` instead of `now_utc()`
   (`crates/nearbee-core/src/journal.rs`), so the replay clock drives
   deadlines and intent staleness (partner); seed the replay clock from the
   real time and only advance it.
4. external_data: a version gate before every start plus a check of every
   observed run detail; on a missing flag abort, decline `failed`, journal a
   privacy incident (partner, refined by the author).
5. Compaction on beta.7 (partner found it): `inspect.rs` knows compaction is
   off only for exactly `1.0.0-beta.6`, so on beta.7 every tier is refused,
   publish and the drift pass included. The author re-read apb `v1.0.0-beta.7`:
   `context_max_bytes` is still set by no surface (every `RunOptions`
   constructor in `apb-cli`, `apb-mcp`, `apb-server`, `judge_eval` and the
   sub-playbook child uses `..Default::default()`; `apb-cli/src/run.rs`
   sets `None`; `RunConfig` is written only by `prepare.rs` from those
   options), so beta.7 is recorded as not compacting too.
6. OpenCode (author's probe 2026-11-11, OpenCode 2.2.5):
   1. `opencode run` without `--standalone` hands the session to a shared
      background service (`opencode serve --service`) that runs with the
      environment of whoever started it; here it ran with the owner's home
      and configuration and connected the owner's `agentic-playbooks` MCP
      server. apb `v1.0.0-beta.7` starts OpenCode as
      `run --format json -m <model> -- <prompt>` (no `--standalone`,
      `crates/apb-core/src/agents/opencode/mod.rs`).
   2. With `--standalone` it honours `XDG_CONFIG_HOME` but still loads
      `~/.agents/skills` and `~/.opencode/skills`; no variable turns that
      off (`OPENCODE_DISABLE_CLAUDE_CODE` covers only `~/.claude`).
   So the attested promise of agent A's fast listing does not hold on a
   machine with OpenCode 2.x, and Nearbee cannot fix it from its side.
7. Options for agent A fast (author):
   1. switch to apb's native `nearai/` route on `dsh`: dsh reads
      `~/.agents/skills`, mounts the home patch layer (where `apb setup`
      registers the MCP server), has no `minimal` environment and no sandbox;
      rejected for the same leak;
   2. ask apb for `--standalone` and a separate home: outside this
      repository and this window;
   3. make the fast variant's model step a Nearbee script node, like agent
      B: `nearbee audit review` packs the snapshot and makes one signed
      DeepSeek call through the S5 signed-call code, then the existing
      `finish` checks every citation. No agent harness on the attested
      tier at all, and the buyer gets a Verification Center receipt.
8. A judge's path (partner): pinned toolchain (exists:
   `rust-toolchain.toml`, 1.99.1), `nearbee replay` independent of config,
   credentials, apb and systemd, CI running it so it cannot rot.

## Decision

1. `nearbee replay` at the `round` level (approach 2) with the journal
   clock fix (approach 3); five scenarios (success, oversized decline,
   redo, restart, cancellation), printed as state names, never ids; CI
   runs it.
2. apb beta.7: compaction known off for beta.6 and beta.7 (approach 5);
   version parsed as semver with prerelease order; serve requires at least
   `1.0.0-beta.7` before every start, sends `external_data: true` on every
   market run and checks it on every observed run detail (approach 4).
   D-023's restriction is lifted once that ships.
3. The attested tier refuses the `opencode` agent (routes unknown, naming
   the shared service), and agent A's fast listing moves to its own
   playbook whose model step is `nearbee audit review` (option 7.3), with
   DeepSeek V4.1 Flash as its one direct and signed model. The premium
   listing keeps the agent playbook on `claude` with `environment: minimal`.
4. README: a replay-first "Try it" on top, apb beta.7 everywhere, the rest
   kept and corrected; a clean-clone run by the integrator.
5. Demo and submission material as docs: shot list, X post draft,
   submission payload draft with the owner's fields left open.

## Rejected

1. HTTP-level replay: mostly invented write answers, already covered by the
   client tests.
2. Moving the test fakes into a shared crate: large refactor, no gain for
   the judge.
3. `dsh` native route for agent A: same leak of the home skills and MCP
   rows, no minimal environment, no sandbox.
4. Allowlisting `OPENCODE_DISABLE_CLAUDE_CODE` (owner item 432): it closes
   only the `~/.claude` source and nothing of the service or `~/.agents`;
   moot once the attested tier refuses OpenCode.
5. A separate `HOME` for the apb unit: breaks the premium listing's Claude
   login and does not stop the OpenCode service.
6. A prebuilt release binary for judges: the README build is one `cargo
   build`; a release is the owner's call and can follow.

## Process

Partner: a fresh Claude Opus context with no access to the author's draft
beyond the plan summary, read-only on the repository; one round. It found
the beta.7 compaction refusal, the journal clock, the capture gaps and the
`~/.agents` source; the author verified each against the code and apb
`v1.0.0-beta.7` and probed OpenCode 2.2.5 live (no model call: a missing
model id fails before any request).
