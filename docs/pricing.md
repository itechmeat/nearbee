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

Every listing file states `price_usd` (USD, decimal string) and
`sla_seconds`. The agent token cannot write the agent or its pricing plans
(`403 agent_token_scope_forbidden`, D-024), so the owner sets the default
`per_call` plan in the market web app; `nearbee list` prints the values to set
and checks the plan it can read.

Optional in the listing file:

- `limits`: `max_brief_bytes` (title plus description, UTF-8 bytes),
  `max_attachments`, `max_attachment_bytes` (the declared sizes of the job's
  attachments, summed) and `file_types` (allowed extensions, case-insensitive,
  with or without the dot). Absent means no limit; an unknown key is refused.
  The limits cover reworks too: the files a change request attaches must have
  an allowed type, and the hire's files over all generations count toward
  `max_attachments` and `max_attachment_bytes`. A rework that breaks a limit
  is declined `failed` before anything is downloaded (the first generation's
  breach is declined `rejected` at intake).
- `scope`: at most 300 characters describing what the agent does, used by the
  scope check below. Absent means the listing's description is used
  (`nearbee list` records the resolved value).
- `protection: off` switches off the cost ceiling, the per-run budget and the
  scope check for a listing whose owner wants every hire run to the end. With
  protection on, a price whose ceiling (`price * 0.95 - 0.25`) is not above
  zero is refused at load (`0.26` refused, `0.264` loads).

`price: auto` (a price computed from the playbook's own run observations: own
p90 cost, a rework reserve, decimal rounding up) is v1.1; the formula from the
design review is kept in `docs/reviews/2026-11-09-codex-design-review.md` and
the earlier version of this file in git history.

## 2. Protection the adapter gives (as built in S3)

Intake runs for a new first-time row before the market start, in this order;
the first check that declines decides, every outcome is journaled without the
brief text, and a decline is `rejected` with a fixed text that ends "You are
refunded in full." (spec `hire-intake`). An owner-started row (started in the
web app before serve saw it) skips intake.

1. **Daily loss limit** (`limits.daily_loss_usd` in `nearbee.yaml`, default
   `"20"`). The sum of the charges of every run that ended without a delivery
   (a decline, an abort, a cancellation, a stray run) on the current UTC day,
   dated by the run's terminal observation, across all hires. At or above the
   limit every new first-time hire is declined ("This agent is not taking new
   work right now.") and the operator is told once per day. The next UTC day
   starts from zero.
2. **Hard limits** of the listing file, checked from the row before any
   download; the text names the limit ("This request is over the listing's
   limit: max_attachment_bytes 10000000.").
3. **Cost admission.** `ceiling = amount * 0.95 - 0.25 USD`, where `amount` is
   the row's `escrowAmount` when above zero (the award, whatever the listing
   file says) and the listing's price for a zero escrow (an owner test run:
   price 0.50 gives 0.225). A ceiling not above zero declines.
4. **Scope check.** One question outside any run to Nearbee's own apb
   instance: `apb decide is <question> --criteria <criteria> --no-run`, the
   brief passed as one argument (no shell) after a fixed text that says to
   treat it as data, with NUL and other control characters (newline and tab
   kept) removed and cut to 32 KiB with a marker, so it always fits one
   argument. Only a clear no (probability of yes below 0.2) declines
   ("This request is outside what this agent does: <scope>."); 0.2 to 0.5 is
   taken and journaled as uncertain. No answer (no provider, a refusal, a
   provider error, the 60 s timeout, a cut-off at another hire's wall clock)
   is journaled as skipped with its reason and the hire goes on;
   `no_provider` is told to the operator once per serve process. A check a
   crash interrupted is skipped as `interrupted` and never asked again, since
   a provider call may cost money. The scope check is the round's one long
   step, oldest hire first.

**Per-run budget.** Each run of a protected hire starts with `budget.usd =
ceiling - spent` (`on_no_estimate: continue`); a hire whose budget is used up
is declined `failed` before a start. `spent` sums the charges of the hire's
runs, every generation and stray included. A run's charge is its reported
cost when that cost is complete (apb's `cost_attempts` equals
`finished_attempts` at the terminal status); a missing or incomplete cost
counts as the run's whole budget, so a run without reported usage leaves less
budget for a rework, never more. A run of an unprotected listing starts
without a budget object and counts its reported cost as is. The terminal
`run_observed` record carries `cost_usd`, `cost_complete` and `budget_usd`, so
the end and its cost land in one record and a crash before the delivery loses
nothing.

apb's budget is a predictive stop before each agent node; a run its budget
gate holds (`budget.paused`) is aborted and the generation declined `failed`.
Nearbee's wall clock (the SLA deadline minus `min(10 min, window / 5)`) is the
hard stop. The owner loses the tokens spent, the buyer is refunded, nothing
half-done is delivered.

One apb run is active at a time across all agents (D-025), so there is no
`concurrent_runs` setting in S3.

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
