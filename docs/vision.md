# Vision

## One sentence

Nearbee lets anyone who runs `apb` playbooks sell them as agents on the NEAR AI
Agent Market, with the buyer's money held by the market on NEAR and the work done by
the models the owner chooses, including TEE models on NEAR AI Cloud.

## Who it is for

1. **Playbook owners** (the seller side). Developers and small teams that already
   encode routines as `apb` playbooks: repository audits, dependency upgrades,
   document digests, release preparation, data cleaning, research briefs. They
   have working automation and spare machine time; they do not have a storefront,
   a payment rail or an agent loop. Nearbee gives them all three with one config
   file.
2. **Buyers on the market.** People hiring from the web or from their AI client
   over MCP, and other agents hiring agent-to-agent over A2A with x402. They want
   a predictable result for a fixed price, a clear statement of where their data
   goes, and a refund if nothing is delivered. The market's held payment gives the
   refund; Nearbee gives the predictability (pre-run intake, SLA from measured
   runs) and the data statement (privacy tiers with evidence).
3. **The apb project.** Nearbee is the first external consumer of apb's server
   API as a product surface. What it needs (run parameters from a brief, result
   extraction, cost estimates, notify hooks) feeds back into apb.

## Why NEAR, concretely

The two hackathon conditions were: a wow effect, and real usefulness on NEAR
rather than NEAR for its own sake. The usefulness:

1. **Payment rail that agents can use without a human.** The market settles
   agent-to-agent hires with x402 in USDC on NEAR mainnet: a NEP-366 signed
   delegate action, gas paid by the relayer, the payment held until delivery. A playbook
   step can hire another market agent and pay for it; another agent can hire a
   Nearbee agent. No Stripe, no API keys, no accounts on the caller's side.
2. **Private AI that can be shown, not promised.** NEAR AI Cloud serves open
   models inside Intel TDX and NVIDIA confidential-computing TEEs, with hardware
   attestation and response signatures. A Nearbee "private" listing runs every
   model step there and attaches the evidence to the deliverable. The buyer can
   verify in the browser (Verification Center) that the model ran in a TEE.
3. **A market that already exists.** 772 live agents, buyers, held payments, disputes,
   reviews. Nearbee does not build a marketplace; it fills one with capable
   agents, which is what the market needs most.

The wow: a playbook that has only ever run on a laptop earns USDC from a
stranger's agent within the demo, and the deliverable carries a proof of where
the data was processed.

## What Nearbee is not

1. Not a marketplace, not a wallet, not a chain client beyond what x402 needs.
2. Not a replacement for apb's dashboard; the owner keeps supervising runs there.
3. Not a hosting service. Runs happen where the owner runs apb. (The market
   itself hosts no agents any more; self-hosting is the norm there.)
4. Not tied to NEAR in code. The market and the payment rail sit behind one
   interface; the name is the only NEAR-specific thing a rename would touch.

## Success criteria for the hackathon

1. A judge can clone the repository, run Nearbee against the public market with
   their own agent token, publish a draft listing from a shipped playbook, hire
   it, and receive a deliverable, following the README.
2. Two example agents are live on the market under the owner's account, one of
   them in the private tier with attestation evidence in its deliverable.
3. The demo video shows an agent-to-agent hire paid with x402 in USDC on NEAR
   mainnet, end to end, including the payout appearing on the owner's balance.
4. The pricing and privacy behaviour described in `docs/pricing.md` and
   `docs/privacy.md` is implemented and exercised: an oversized brief is declined
   with a full refund; a private listing refuses to publish with a non-TEE profile.
