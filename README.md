# Nearbee

**Your playbooks, for hire on NEAR.** Nearbee lists
[Agentic Playbooks](https://github.com/itechmeat/agentic-playbooks) (`apb`) as
agents on the [NEAR AI Agent Market](https://market.near.ai), runs every hire as a
playbook run on the owner's machine (or with TEE models on NEAR AI Cloud), and
delivers the result back to the buyer, who pays per job in USDC on NEAR.

Status: design complete, implementation starting (NEAR Hackathon, 2026-11-01 to
2026-11-14). See `docs/plan.md`.

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
- Private AI: the "private" listings run their model steps on NEAR AI Cloud TEE
  models and attach the attestation evidence to the deliverable (the exact
  evidence level is defined in `docs/privacy.md`).
- Agent economy: a Nearbee agent can itself hire other market agents from inside
  a playbook step, paying with x402.

## Try it

Implementation in progress. The steps below are the target for the judges'
walkthrough and will be kept exact as the code lands.

1. `cargo build --release` (Rust stable; `apb` installed and `apb dashboard` running).
2. Put your market agent token and apb address into `.env` (see `.env.example`).
3. `nearbee list playbooks/repo-audit/listing.fast.yaml` - publishes a draft listing.
4. Hire your own draft agent on its market page with a short brief.
5. `nearbee serve` - watch the hire become a run and a deliverable.
6. `cargo run -p x402-hire -- <handle>` - hire it agent-to-agent with USDC.
7. `nearbee replay` - the same lifecycle on recorded responses, no accounts needed.

## Hackathon

Tracks: Agent Economy (`agents`), Privacy & Private AI (`privacy`), Autonomous
Business (`business`). Baseline before the window: `apb` itself (a separate
repository, pre-hackathon). Everything in this repository was created inside the
window. Rules and the submission contract: `docs/hackathon.md`.

## License

MIT, see `LICENSE`.
