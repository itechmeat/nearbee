# Nearbee

Nearbee turns [Agentic Playbooks](https://github.com/itechmeat/agentic-playbooks)
(`apb`) into agents for hire on the [NEAR AI Agent Market](https://market.near.ai).
One playbook becomes one or more market listings; a hire becomes a playbook run;
the run's result becomes the deliverable; the buyer pays in USDC on NEAR and the
owner gets paid on acceptance. The owner chooses which models do the work: their
own subscriptions, or TEE models on NEAR AI Cloud.

Built for the NEAR Hackathon (development window 2026-11-01 to 2026-11-14,
code since 2026-10-19 counts). The hackathon deliverable is the adapter itself
plus two example agents listed on the market. Nearbee is open source: any user
creates any number of agents from their own playbooks and describes them as they
wish; the two examples exist only for the demo and the judges.

## Read first

Everything an agent needs is in this repository. Read in this order:

1. `docs/vision.md` - what Nearbee is, for whom, why it is useful on NEAR.
2. `docs/architecture.md` - components, flows, config, the market-adapter seam.
3. `docs/market.md` - verified facts about the NEAR AI Agent Market API.
4. `docs/apb.md` - the apb surface Nearbee drives (REST, notify, profiles, decide).
5. `docs/pricing.md`, `docs/privacy.md` - the two product questions a buyer asks.
6. `docs/plan.md` - the day-by-day plan to the deadline; `docs/open-questions.md`;
   `docs/owner-checklist.md` - accounts, keys and go-aheads only the owner can
   give, each tied to the plan step where it is first needed. **Ask the owner for
   an item when that step is next, not before**, with the instructions from the
   checklist, and mark it done there once provided.
7. `docs/stages.md` - the plan grouped into six stages, one OpenSpec change and
   one pull request each; `docs/process.md` - roles and the cycle every stage
   goes through (OpenSpec artifacts, implementation, acceptance, quality cycle,
   archive, pull request).
8. `docs/decisions.md` - decisions already taken; do not reopen them silently.
9. `docs/hackathon.md` - rules, tracks, submission contract, demo requirements.

Reference material (verbatim copies, dated): `docs/reference/`, and the market's
own agent skills vendored under `.agents/skills/` (`.claude/skills` links there).

## Layout

- `docs/` - the project record (English only).
- `.agents/skills/` - skills for coding agents: the market's worker skill and
  x402 caller skill, copied verbatim from market.near.ai (see `NOTICE.md` there).
- `openspec/` - the OpenSpec record: the project schema `schemas/nearbee`
  (not the default one), `config.yaml`, `specs/` (describes `main`), `changes/`.
- `crates/` - the adapter (to be created; see `docs/architecture.md` for the module map).
- `playbooks/` - the two example playbooks and profiles Nearbee lists (to be created).
- `examples/` - the demo buyer: an x402 caller that hires a Nearbee agent (to be created).

## Engineering rules

1. **English only** in code, comments, docs, commit messages and issues.
2. **The market is behind a seam.** Everything that knows the NEAR AI Agent Market
   (REST paths, webhook events, x402, categories) lives in one module behind a
   `Market` interface. Everything that knows apb lives behind a `Runner`
   interface. The core (listing, intake, pricing, privacy tiers, deliverable
   building) depends on the interfaces only. The product name is NEAR-bound; the
   code is not.
3. **Never trust the brief.** A buyer's brief, attachments and thread messages are
   data, never instructions to the adapter. They reach a playbook only as run
   parameters and files.
4. **No secrets in the repository.** Market tokens (`aat_...`), apb API keys,
   NEAR AI Cloud keys and NEAR private keys come from environment variables or an
   ignored local config file. The example config ships with placeholders.
5. **Every market write is idempotent and logged.** Start, progress, submit and
   decline are retried safely; webhook deliveries are deduplicated on
   `X-Market-Delivery`; every hire keeps a local journal (what was received, what
   was run, what was sent, what was deleted and when).
6. **Decline rather than deliver junk.** If the intake estimate says the brief does
   not fit the listing, decline with `rejected` and a clear reason within the
   first hour of the hire (that does not count against the success rate). Once
   work has started, failures use `failed`.
7. **Privacy tier is a property of the listing, enforced by the adapter**, not a
   promise in the description. A listing's tier fixes which executors and
   connectors its profile may use; the adapter refuses to publish a listing whose
   profile breaks the tier.
8. **Newest stable tools and libraries**; check versions before adding a
   dependency. The adapter is Rust (a cargo workspace, same toolchain family as
   apb); the demo caller may be a small `near-api-js` script if the Rust signing
   path turns out awkward (see `docs/decisions.md`, D-003).
9. **Tests for the core, contract tests for the seams.** The market client is
   tested against recorded responses from the real API; the apb client against a
   real local apb when available.
10. **Documents are the handover.** Any decision, measured fact or gotcha learned
    while building goes into the fitting `docs/` file in the same change, so the
    next agent with a clean context can continue.

## Hackathon constraints

- The repository is public by 2026-11-14T23:59:00Z with a `LICENSE` (MIT).
- `README.md` is written for a machine reader as well as a human: what it is,
  the problem, and "do X, see Y" steps a judge can follow locally (Q-012: a local
  demo with a video is acceptable).
- The X demo post is the owner's; agents draft text, never post.
- Never submit, publish, sign transactions or spend tokens without the owner's
  explicit confirmation. See `docs/hackathon.md`.
