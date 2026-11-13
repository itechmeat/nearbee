<p align="center"><img src="brand/poster/banner.png" alt="Nearbee: your playbooks, for hire on NEAR. Nearbee lists a playbook on the NEAR AI Agent Market, runs each hire on the owner's machine, delivers the signed result and the owner is paid in USDC" width="100%"></p>

# Nearbee

**Your playbooks, for hire on NEAR.** Nearbee lists
[Agentic Playbooks](https://github.com/itechmeat/agentic-playbooks) (`apb`) as
agents on the [NEAR AI Agent Market](https://market.near.ai), runs every hire as a
playbook run on the owner's machine (or with TEE models on NEAR AI Cloud), and
delivers the result back to the buyer, who pays per job in USDC on NEAR.

Status: stage S6 of six (NEAR Hackathon, 2026-11-01 to 2026-11-14): listing,
serving with a durable journal, privacy tiers, the two example agents and the
credential-free replay are implemented; the live runs of the demo are
recorded in `docs/demo-log.md`. See `docs/stages.md`.

## The problem

Teams already automate repeatable work with playbooks: audit a repository,
digest a batch of documents, prepare a release, write a report. Those playbooks
earn nothing while the machine idles, and the people who would pay for exactly
that routine cannot find or hire it. The NEAR AI Agent Market has the buyers,
the held payment and the payment rail, but it expects "an agent" behind an HTTP loop,
and most teams do not have one.

## The product

Nearbee is that agent, for any playbook:

1. **List.** One command turns a playbook into a market listing: name, tagline,
   input schema, the owner's price and delivery window. One playbook can be
   listed several times with different model profiles and prices (fast,
   premium, private).
2. **Serve.** Nearbee polls the market, checks each brief against the listing
   before accepting it, starts the run with a cost ceiling derived from the
   price, posts progress, and submits the result as Markdown plus files. Rework
   requests become follow-up runs; a run that would exceed the ceiling is
   stopped and the buyer refunded.
3. **Choose the models.** A listing's profile decides who does the work: the
   owner's own agent subscriptions, or NEAR AI Cloud models running inside TEEs
   with attestation, so the buyer gets evidence of where their data went.
4. **Get paid.** Buyers pay into the market's held payment, from the web, from
   an MCP client, or agent-to-agent over A2A with x402 micropayments in USDC on
   NEAR mainnet; the payment is released on acceptance. The owner withdraws
   USDC to any wallet.

## How NEAR fits

- Settlement: USDC on NEAR through the market's held payment, including
  agent-to-agent hires paid with x402 (NEP-366 meta-transactions, the relayer
  pays gas).
- Private AI: the attested listings run no agent: every model call is a
  direct call of Nearbee's own commands to a NEAR AI Cloud TEE model, one
  call per hire carries the model TEE's response signature with a
  Verification Center receipt, and the deliverable discloses the endpoint
  attestation reports (the exact evidence level is in `docs/privacy.md`).
- Agent economy: a Nearbee agent can itself hire other market agents from inside
  a playbook step, paying with x402.

## Try it

### 1. See it work without any account

With Rust stable (the toolchain is pinned in `rust-toolchain.toml`) and
nothing else: no config, no keys, no apb, no network.

```sh
cargo run -p nearbee -- replay
```

It walks five scripted hires (success, a decline at intake, a redo, a
restart after a lost answer, a cancellation) through Nearbee's real serve
round, journal, intake, tier check, manifest validation and signature
recheck, against a scripted market, runner and attestor. It says so in its
first lines: the market rows are shaped like recorded market answers, the
transitions are scripted, nothing is contacted, and the response signature
in the deliverable is a synthetic placeholder that will not verify. The
last lines are:

```text
success       ok
decline       ok
redo          ok
restart       ok
cancellation  ok
replay: 5 of 5 scenarios ok
```

A cold build and run took 16 s on a 16-core machine with the crates already
downloaded. The first build downloads the crates, and a slower machine takes
a few minutes. `--scenario restart` runs one scenario, `--verbose` names the
journal events of each step, `--keep <dir>` keeps the state (a new or empty
directory). CI runs the replay on every push.

### 2. Run it live

With Rust stable and `apb` 1.0.0-beta.7 or newer installed:

1. `cp .env.example .env` and set `NEARAI_API_KEY` (needed by the attested
   listings' model calls; the checks below report it by name, never its
   value).
2. `scripts/apb-instance.sh start` starts Nearbee's own apb dashboard on port
   7322 (it never touches an apb you already run on 7321); then
   `scripts/apb-instance.sh register ~/.local/share/nearbee/apb-workspace --git`
   and `scripts/apb-instance.sh install` create the dedicated workspace with
   the profiles and the smoke playbooks (`docs/apb.md`).
   To run the instance in a container instead (Docker; the steps see only
   the workspace and the instance's own directories, and reach only the
   hosts in `sandbox/egress.allow`), build the image with
   `sandbox/build.sh`, set `NEARBEE_APB_SANDBOX=1` for the same commands
   and `APB_BIN=$PWD/scripts/apb-sandboxed` for `nearbee`. It is one
   container per instance, not per hire, and filters egress by TLS server
   name; `docs/apb.md`, "Sandboxed instance", lists the limits.
3. `cargo run -p nearbee -- config check` prints one line per check (`ok`,
   `missing`, `failed`): the config file, each variable by name, the apb
   dashboard, the workspace and the apb version (Nearbee needs
   `1.0.0-beta.7` or newer: every market run is an apb external-data run).
   Exit 0 when everything required is there.
4. `NEARBEE_LIVE=1 cargo test -p nearbee-apb --test live -- --ignored live_smoke_own --nocapture`
   runs the synthetic smoke playbook through Nearbee's runner on your own
   agent subscription and validates its result manifest.
5. `cargo run -p nearbee -- runs find --execution-id <id printed by step 4>`
   prints the run and `complete: yes` (exit 0); exit 1 means no run and the
   search was complete, exit 2 no run but some runs could not be searched.
6. Store your own market agent token (shown once in the web app; it goes to
   standard input, never to a flag or the environment):
   `nearbee agent add <handle>` with the token on standard input. A draft
   agent answers 404 to every public lookup, so its id cannot be resolved
   from the handle: copy the agent id from the agent's settings page in the
   market web app and run `nearbee agent add <handle> --agent-id <uuid>`
   (a live agent resolves without the flag). It is kept
   in `~/.config/nearbee/credentials` with mode 0600 and printed only as
   `aat_...` plus its last four characters. `nearbee agent list` shows the
   handles with masked tokens. Set `NEARBEE_CREDENTIALS` to use a different
   store file (an operator override, honoured by `agent`, `list` and
   `serve`); the default stays under `XDG_CONFIG_HOME`.
7. `nearbee list playbooks/repo-audit/listing.repo-audit-fast.yaml`
   installs the listing's playbook into the apb workspace, checks its
   privacy tier, records the installed version and prints a table of the
   values you set in the market web app (name, description, bio,
   category, tags, SLA, price), each with whether the market already
   matches, and the generated data statement for the bio. The fast audit
   is `attested`: its playbook `playbook.fast.yaml` has script nodes only
   (fetch, review, finish) and no agent; its one model call is `nearbee
   audit review`, a signed call to `deepseek-ai/DeepSeek-V4.1-Flash-1017`
   on NEAR AI Cloud, the listing's one direct and signed model. An agent
   token cannot change the market agent (it answers `403
   agent_token_scope_forbidden`), so `nearbee list` sends no market
   write. The listing stays a draft until you publish it in the web app.
8. `nearbee serve` watches the market for your draft agent's hires and
   turns each one into an apb run and a submitted deliverable: intake
   (daily loss limit, the listing's hard limits, the cost ceiling, the
   listing's intake check, a scope check), the buyer's attached files, the
   run with a budget as an apb external-data run, and later a rework or a
   dispute redo as a continued run with the buyer's feedback. Every step
   is journaled under `.nearbee/hires/` and resumed after a restart; one
   run is active at a time and one `serve` holds `.nearbee/serve.lock`.
   `nearbee serve --reject <handle>` declines every new first-time hire of
   that agent with `rejected` before any work (the no-spend x402 proof).
9. `nearbee hires` lists the journaled hires (state, latest run, spent
   cost, deadline, an attention code when one needs you; `--json` for
   JSON), read only. `nearbee cleanup --dry-run` prints which hires'
   files would be deleted and what Nearbee cannot delete; `nearbee
   cleanup` deletes them under the serve lock and journals what it removed
   (`docs/privacy.md`).
10. `cd examples/x402-hire && npm install && node index.mjs <handle> "<task>"
    --dry-run` reads an agent's 402 challenge, signs the NEP-366 delegate
    with `near-api-js` v7, verifies it locally and stops without sending
    (needs `NEAR_CALLER_ACCOUNT_ID` and `NEAR_CALLER_PRIVATE_KEY` in
    `examples/x402-hire/.env`; the key is never printed).

The live round trip (a real hire on the draft agent, and the paid x402
decline proof) needs the owner's accounts and is exercised in the demo;
see `docs/demo-log.md` once run.

## Hackathon

Tracks: Agent Economy (`agents`), Privacy & Private AI (`privacy`), Autonomous
Business (`business`). Baseline before the window: `apb` itself (a separate
repository, pre-hackathon). Everything in this repository was created inside the
window. Rules and the submission contract: `docs/hackathon.md`.

## License

MIT, see `LICENSE`.
