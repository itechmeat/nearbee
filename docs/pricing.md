# Pricing and intake

## The constraint

The market charges **per completed job at the listing's one default price**
(`per_call`), in USD paid 1:1 in USDC. Buyers cannot pick a plan or a price on
an instant hire or an x402 call. `per_task` with parameters and subscriptions
are reserved in the API for a later version. Only board bids carry a custom
price. Nothing is charged for a declined, failed or cancelled job, and the
platform keeps 5% of the price on success.

So the seller takes the cost risk per job. Nearbee cannot know what an
arbitrary playbook can handle; only its author can. So v1 keeps pricing in the
owner's hands and puts the protection in the adapter, where it needs no
configuration.

## 1. The owner sets price and SLA (v1)

Every listing file states `price` (USD, decimal string) and `sla` (minutes).
`nearbee list` publishes them as the default `per_call` plan. That is all the
pricing an owner has to do.

Optional in the listing file:

- `limits`: attachment count and total size, page count, file types, brief
  length, repository size for repository listings. Absent means no limit.
- `scope`: one or two sentences describing what the agent does, used by the
  scope check below. Absent means the listing's description is used.
- `protection: off` switches off the cost ceiling and the scope check for a
  listing whose owner wants every hire run to the end.

`price: auto` (a price computed from the playbook's own run observations: own
p90 cost, a rework reserve, decimal rounding up) is v1.1; the formula from the
design review is kept in `docs/reviews/2026-11-09-codex-design-review.md` and
the earlier version of this file in git history.

## 2. Protection the adapter gives without configuration

1. **Scope check before start.** One cheap question to a decision model
   (`apb decide is`): "Is this brief a request for <scope>?" A clear no →
   decline with `rejected` (free of reputation cost in the first hour), a plain
   reason and the full-refund fact. Hard limits, when the listing has them, are
   checked first.
2. **Per-hire cost ceiling.** `ceiling = price * 0.95 - 0.25 USD`, cumulative
   over the first run and every rework or redo run of the hire. Each run starts
   with `budget.usd = ceiling - spent` (`on_no_estimate: continue`) and a
   wall-clock limit at the SLA minus a margin. apb's budget is a predictive stop
   before each agent node, so Nearbee's wall-clock limit is the hard stop. On
   overrun the run is aborted and the hire declined with `failed`: the owner
   loses the tokens spent, the buyer is refunded, nothing half-done is
   delivered.
3. **Global limits** in `nearbee.yaml`: `concurrent_runs` and `daily_loss_usd`
   (stop taking hires for the day when failed-run spend passes it). Defaults 2
   and 20.

## 3. One playbook, several listings (SKUs)

A listing has one price and the apb REST start has no version or profile
override, so variants are **materialised**: `nearbee list` copies the source
playbook into a variant playbook with fixed profile bindings, registers it, and
publishes it as its own market agent (own handle, own token, own price). The
description says what differs: models, evidence level, limits.

| Variant suffix | Profile set | What the buyer gets for the price difference |
| --- | --- | --- |
| `fast` | NEAR AI Cloud Confidential TEE open models | TEE models, run-level attestation disclosure |
| `premium` | owner's frontier subscription (Claude, GPT) | stronger model; no TEE claim |
| `private` | TEE-only profiles, no connectors, evidence block, signed step where the playbook has one, shorter retention | the enforced data statement and evidence |

Parameters that change cost little (language, tone, output format) ride inside
the one price as `input_schema` fields.

## 4. Board bids (v1.1)

The board is the only place with a brief visible before the price. A later
version can watch `GET /v1/jobs/board`, run the scope check on the brief and bid
with a computed `amount` and `slaSeconds`. Out of scope for the hackathon.

## Worked example

Listing "Repository audit · fast", owner-set price USD 3.00, SLA 30 min,
limit "public repositories up to 100 MB". Ceiling per hire 2.60. A hire with
a 400 MB monorepo is declined in 20 s: "this listing audits public
repositories up to 100 MB; split the request or use repo-audit-premium",
buyer refunded in full, success rate untouched. Another hire runs, spends
1.40, gets a rework request, the rework runs with budget 1.20 and spends 0.60;
accepted; payout 2.85, cost 2.00.
