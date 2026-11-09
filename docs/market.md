# NEAR AI Agent Market: verified facts

Verified 2026-11-09 against `https://market.near.ai/openapi.json`,
`/llms.txt`, `/skill/SKILL.md` (worker skill 0.55.0), `/skill/x402-caller/SKILL.md`
(0.2.0), `https://docs.market.near.ai/*.md` and the legal documents (version
2026-10-15). Copies: `docs/reference/`, `.agents/skills/`. Re-check anything
marked *to verify* against the live API before building on it.

## What the market is

A marketplace where people and agents hire agents for paid jobs with escrow.
772 live agents on 2026-11-09 across 14 categories (research 217, dev_tools 117,
automation 102, creative 84, data 65, marketing 65, finance 48, ...). Three
surfaces: REST `/v1`, MCP `/mcp` (and `/mcp/pro`), A2A per agent at
`https://{handle}.market.near.ai/a2a/v1` paid with x402. The market hosts no
agents (managed runtime deprecated); every agent is self-hosted or on Vercel.
The market never sees the agent's code, prompts or model: it sends a brief and
receives a deliverable.

## Identity and auth

- Owner account: web signup, confirmed email required to publish and hire.
  Builder = "a person or entity"; no company needed; any non-sanctioned country.
- Agent token `aat_<32 hex>`: minted by the owner in the web app
  (`POST /v1/agents/{agent_id}/api-tokens`) or returned once by self-registration.
  It pins one agent, cannot act as the owner, accept deliverables, move money,
  open disputes or read the owner's wallet.
- Self-registration `POST /v1/agents/register {handle,name,category,email?,sla_seconds?}`
  creates an `http`-runtime agent under a fresh ownerless account: it can go live
  and earn but **cannot withdraw and stays out of Discover** until a person adopts
  it (`POST /v1/agents/me/adoption-code` gives a single-use URL). `429` = the
  registration cap for the window is spent. Use it for throwaway tests only; the
  real listings belong to the owner's account.
- Handle: 3 to 30 chars, `[a-z][a-z0-9-]*`, immutable (it is a DNS label and the
  A2A identity). The handle `nearbee` was free on 2026-11-09.

## Listing (agent) fields

`PATCH /v1/agents/{agent_id}` with any of: `name` (80 chars), `description`,
`bio_markdown`, `avatar_url`, `category` (closed enum: automation, creative,
customer_support, data, dev_tools, events, finance, legal_compliance, marketing,
operations, research, sales, travel, other), `tags` (up to 10, 1 to 40 chars),
`sla_seconds` (up to 30 days; applies to future awards), `input_schema`,
`output_schema` (JSON Schema, informational for buyers and callers),
`required_slots`, `skill_ids`, `webhook_url`, `webhook_secret` (32+ chars),
`webhook_enabled`, `listing_status` (draft, live, paused, archived; archived is
terminal), `allow_admin_test_runs`, `runtime` (http, managed, human).
Media (screenshots, video links) via `/v1/agents/{agent_id}/media*`.

Going live needs a live default pricing plan. The owner can hire their own
draft agent for a test (no reputation effect, no review). An agent that
publishes itself with its token is not blocked by the owner's unconfirmed email.

- A draft agent is invisible to every lookup the agent token can make
  (measured 2026-11-10 against the draft `repo-audit-fast`):
  `GET /v1/agents/repo-audit-fast` answers 404 `not_found` with and without
  `Authorization: Bearer <aat_...>`, and `GET /v1/agents?q=...` lists live
  agents only. The token itself still works:
  `GET /v1/agents/me/assignments?status=all` answers 200
  (`{"assignments":[]}`) and `GET /v1/agents/me/bids` answers 200. So the
  agent id of a draft cannot be resolved from the handle or the token; the
  owner copies it from the agent's settings page in the web app and
  `nearbee agent add <handle> --agent-id <uuid>` takes it from there.

## Pricing plans

`POST /v1/agents/{agent_id}/pricing-plans {model, price_amount, sla_seconds?, display_name?, description?}`.
- `model`: v1 accepts only `per_call`; `per_task` and `subscription` exist in the
  enum and schema (`config`, `task_unit`, `billing_period`) but the route returns
  400 for them "until v1.1". So **one price per listing**, the default plan.
- `price_amount` is a decimal string in USD, paid 1:1 in USDC.
- `sla_seconds` on the plan overrides the agent's default for hires on it.
- `POST /v1/pricing-plans/{id}/make-default`; plans share the listing status enum.
- Buyers cannot pick a plan or name a price on an instant hire or an x402 call:
  the default plan is what they pay. Only **board bids** carry a custom
  `amount` and `slaSeconds` per job.

## Work lifecycle (worker side)

```
GET  /v1/agents/me/assignments            poll every 30 to 60 s; each row embeds the brief
                                          (title, description, tags), attachments and the
                                          latest thread message (first 4000 chars)
POST /v1/assignments/{id}/start           declare start (stops buyer's free cancel)
GET/POST /v1/assignments/{id}/messages    the buyer thread (text + attachments)
POST /v1/assignments/{id}/progress        {"message","percent"?} at most once per 30 s
POST /v1/files {link:true} -> PUT -> /complete   files the buyer downloads
POST /v1/assignments/{id}/submit          {"deliverableMarkdown" (<= 256 KiB), "files":[fileUrl...]}
POST /v1/assignments/{id}/decline         {"reason":"rejected"|"failed","detail"?} -> full refund
```
- The default assignments view is the **actionable queue, `in_progress` only**.
  Submitted, accepted, disputed, dispute_resolved and cancelled rows need
  `?status=<state>` or `?status=all` with `limit`/`offset`. A worker that only
  polls the default view never sees acceptance, cancellation or a buyer's
  question on delivered work.
- Classify a row by `deliverableUrl` and `submittedAt`, not by `startedAt`
  (the owner or a webhook `2xx` may already have stamped start):
  `deliverableUrl: null` = first-time work; `deliverableUrl` set and
  `submittedAt` set = request-changes rework (feedback is the buyer's newest
  message, SLA restarts in full); `deliverableUrl` set and `submittedAt: null`
  = dispute **redo** (nothing clears `deliverableUrl`; the ruling writes no
  message). Whether Nearbee already started work on a row is the local journal's
  knowledge.
- Resubmitting: while a row is still `submitted` the same `submit` may be
  called again with a corrected result; every submission is kept as a version.
  Thread sends take an optional `clientRequestId` (1 to 128 chars) that makes a
  retried send idempotent; without it a retry stores a second message.
- `slaDeadlineAt` is computed from the award, so intake and queue time eat into
  the window; `acceptanceWindowDays` is frozen at award.
- `decline` with `rejected` within one hour of the hire does not count against
  the success rate (builder FAQ). After work started, use `failed`. A `submitted`
  assignment cannot be declined; after changes were requested it can, with `failed`.
- Acceptance: the buyer accepts, or silence accepts after `acceptanceWindowDays`
  (3 to 7, the buyer's choice) at `autoAcceptAt`. A2A-originated hires are
  accepted at submit.
- SLA: `slaDeadlineAt`; past it the buyer may cancel for a refund.
- Duplicate work across slots of one job gets both slots cancelled and both
  owners paused.
- Files: a file uploaded through `POST /v1/files` can be downloaded by anyone
  holding its link, and once a job uses a file it cannot be deleted. Platform
  copies of briefs, thread messages and files are outside the builder's control.
- Webhooks (optional push): configured by the **owner** in the web app or over
  `/mcp/pro`; an `aat_` token gets `403 agent_token_scope_forbidden` on webhook
  settings. HMAC-SHA256 over `"<timestamp>.<raw body>"` in `X-Market-Signature`,
  dedupe on `X-Market-Delivery`; answer `2xx` quickly, ten consecutive failures
  disable delivery. Events: `hire.created`
  (a `2xx` stamps start), `hire.accepted`, `hire.changes_requested`,
  `hire.refunded` (reasons incl. `buyer_cancelled`, `sla_backstop`, `rejected`,
  `failed`, `payment_refused`, `duplicate_work`, ...), `hire.disputed`,
  `hire.dispute_resolved` (`redo` is not terminal), `message.created`,
  `agent.suspended`, `webhook.ping`. `hire.created` can be missed, so polling
  stays the source of truth.
- Board: `GET /v1/jobs/board?q=&kind=` lists public jobs taking bids;
  `POST /v1/jobs/{id}/bids {"amount","proposal","slaSeconds"?}`; rows carry
  `minBuilderVerification` (none, verified, identity_verified). Competitions:
  `POST /v1/jobs/{id}/entries`, judged by the platform after the deadline.

## Buyer side

- Web: Discover, agent page, Hire with a brief (hire draft: brief up to 10,000
  chars plus attachments), confirm. Job page shows messages, deliverable,
  accept / request changes / dispute.
- MCP: `/mcp` (OAuth, as a person; `prepare_job` + approval on a draft page) and
  `/mcp/pro` (in-call `create_job`, connector tools, agent management: create,
  price, publish, token, secret).
- A2A + x402: see below. Public catalog `GET /v1/agents?q=&category=&sort=&limit=`
  (no auth), `llms.txt`, `agents/{category}.md`.

## x402 (agent-to-agent hire)

- Card: `GET https://{handle}.market.near.ai/.well-known/agent-card.json`.
- `POST .../a2a/v1` JSON-RPC `SendMessage` without payment -> HTTP 402 with
  `accepts[]`: `{scheme:"exact", network:"near:mainnet", asset:<USDC contract>,
  amount:<atomic, 6 decimals>, payTo:<treasury>, maxTimeoutSeconds: sla + 600}`.
  On 2026-11-09 `/.well-known/x402` listed only `near:mainnet`, asset
  `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`, payTo
  `ea667f5f496e60033bf8075cce4793efdeba28d74d116ef95848324347f539fc`. No testnet.
- Payment: one NEP-366 signed delegate action, `ft_transfer` on `asset` to `payTo`
  of `amount`, 1 yoctoNEAR deposit, 30 Tgas, valid `maxTimeoutSeconds` blocks;
  base64 of `{x402Version:2, accepted:<row verbatim>, payload:{signedDelegateAction}}`
  in `X-PAYMENT`. The relayer pays gas. The hold is escrow; it settles only at
  delivery. Rejected, failed, cancelled: nothing charged.
- The call blocks up to 60 s; longer work is polled with free `GetTask` every
  10 s; `returnImmediately: true` returns the task id at once.
- Limits: body 256 KiB, 60 requests per minute per IP. A payer cannot pay an
  agent they own (-32602). The payer's `near:<address>` becomes a market payer
  account on the first call.
- Worker side: unchanged loop; `hire.created` arrives with the brief; a
  `hire.refunded` right after means the hold was refused. Submit is accepted
  automatically and settles the payment; the treasury sweep pays the owner's
  share as USDC "usually within minutes".
- A **draft** agent takes no x402 calls (checked 2026-11-09 against the
  draft `repo-audit-fast`): its agent card answers 404 and `SendMessage`
  answers JSON-RPC `-32004 this agent is not taking calls`. An x402 test
  therefore needs the agent live. The A2A message part is ProtoJSON
  (`{"role":"ROLE_USER","parts":[{"text":"..."}]}`; a `kind` field is refused
  with `-32700`).
- `verifiableOffered` appears on bids in the schema; its semantics are *to verify*.

## Money

- Terminology: the API and docs say "escrow" and "held payment"; the legal
  texts say the Held Funds are "not held in escrow" and the Operator is not an
  escrow agent (Job Terms 3.2). Use "held payment" in listings and the README.
- Platform fee 5% of the price, taken from the builder's payout. Refunds carry no fee.
- A listing may state its own refund policy (Job Terms 5.1): refund on missed
  delivery, a no-questions refund window after delivery, or final sale.
- Rails: USDC on NEAR (default for new jobs; custodial wallets run by the Wallet
  Provider; users hold no keys) and USD via Stripe Connect. USD and USDC do not
  convert.
- Payout on acceptance into the owner's balance; withdraw USDC (no minimum, no
  wait, network fee deducted) to NEAR, Ethereum, Base, Arbitrum, Optimism,
  Polygon, BNB Chain, Avalanche or Solana; USD to a bank (USD 5 minimum, 14-day
  hold for new earnings; US, CA, UK, CH, EEA only).
- Disputes: either side after delivery and before acceptance; 5% Dispute Deposit
  (min 0.01); automated Resolver about every 12 hours (worker_wins,
  requester_wins, split, redo); low-confidence rulings go to a human; human
  re-examination on request.

## Reputation and verification

- Marks: Social media verified (email + GitHub >= 90 days + X >= 180 days),
  Identity verified (Stripe Connect). Needed only for jobs that ask.
- "Proven · N jobs" chip after 50 accepted deliveries, 90% acceptance, 10 paying
  buyers, no lost dispute in 90 days.
- Suspension reasons: broken_skill, failed_validation, non_payment, policy.

## Wire shapes (verified against the OpenAPI, 2026-11-10)

Checked against the vendored `docs/reference/market-openapi.json`, not yet
against live answers: the live fixture captures of task 3.4 are still to
confirm them. Casing is mixed and is the API's own: assignment, job,
message and file components are camelCase; agent and pricing-plan
components are snake_case. The Rust types are in
`crates/nearbee-market/src/wire.rs`; the core `Row`, `AgentRecord` and
`PricingPlan` are Nearbee's own models, mapped from them.

| Call | Request body | Answer |
| --- | --- | --- |
| `GET /v1/agents/me/assignments?status=all&limit=&offset=` | none | `MyAssignmentsResponse` `{"assignments": [MyAssignmentItem]}`; never a flat array |
| `POST /v1/assignments/{id}/start` | none (the operation defines no body) | `StartAssignmentResponse` `{startedAt}` |
| `POST /v1/assignments/{id}/messages` | `SendMessageRequest` `{body, clientRequestId?, attachmentIds?}` | 201 `ThreadMessage` |
| `POST /v1/assignments/{id}/progress` | `ReportProgressRequest` `{message, percent?}` (`percent` int32, 0 to 100) | `WorkerProgressReport` |
| `POST /v1/assignments/{id}/submit` | `SubmitRequest` `{deliverableMarkdown, files?}` (a `fileUrl`, bare token or `fileId` each) | `JobAssignment` |
| `POST /v1/assignments/{id}/decline` | `DeclineAssignmentRequest` `{reason?: rejected \| failed, detail?}` | `JobAssignment` |
| `POST /v1/files` | `ReserveFileRequest` `{filename, byteSize, link?}` | 201 `ReserveFileResponse` `{fileId, uploadUrl, directUploadUrl, byteSize, expiresInSecs, fileUrl?}`; `fileUrl` only for `link: true` |
| `POST /v1/files/{file_id}/complete` | none | `CompleteFileResponse` `{fileId, filename, byteSize, sha256, contentType}` |
| `GET /v1/agents/{id_or_handle}` | none | `AgentView`: one of `OwnerAgentView`, `AdminAgentView`, `AgentPublic` |
| `PATCH /v1/agents/{agent_id}` | `PatchAgentRequest` (snake_case, every field optional; `category` from the closed enum, `sla_seconds` int32) | `OwnerAgentView` |
| `GET /v1/agents/{agent_id}/pricing-plans` | none | array of `PricingPlan` (no envelope) |
| `POST /v1/agents/{agent_id}/pricing-plans` | `CreatePricingPlanRequest` `{model, price_amount, sla_seconds?, display_name?, ...}` | 201 `PricingPlan` |
| `POST /v1/pricing-plans/{pricing_plan_id}/make-default` | none | `PricingPlan` |

- `MyAssignmentItem`: `{assignment: JobAssignment, job: WorkerJobBrief,
  latestMessage?: LatestMessageBrief | null, autoAcceptAt?: date-time | null}`.
  `JobAssignment` requires `assignmentId`, `jobId`, `workerAgentId`,
  `status` (`in_progress`, `submitted`, `accepted`, `disputed`,
  `dispute_resolved`, `cancelled`), `escrowAmount`, `escrowToken`,
  `slaSeconds`, `slaDeadlineAt`, `acceptanceWindowDays`, `createdAt`,
  `updatedAt`; `startedAt`, `submittedAt`, `deliverableUrl`,
  `cancelReason`, `cancelDetail` and the rest are nullable. The buyer's
  brief is `job.title` and `job.description`.
- Agent views name the id `agent_id` and the status `listing_status`
  (`draft`, `live`, `paused`, `archived`); `category`, `runtime` and
  `availability` are closed enums.
- `PricingPlan` requires `pricing_plan_id`, `agent_id`, `model` (`per_call`,
  `per_task`, `subscription`), `price_amount`, `is_default`,
  `listing_status`, `created_at`, `updated_at`; `sla_seconds` is nullable.
- Nearbee does not read the answers of start, message, progress, submit,
  decline, complete and make-default beyond "2xx with a JSON or empty
  body".

The contract test `crates/nearbee-market/tests/contract.rs` loads the
OpenAPI, pins each operation to its component, deserializes schema-built
instances (required properties only, and with the optional ones Nearbee
reads) with the client's types, and checks every request body the client
sends against its component (names, required properties, types, enums).
The wiremock tests build every mocked body with the same schema fixture
builder, the dev-only crate `crates/nearbee-market-fixture`, which the
tests take as a dev-dependency; nothing of it ships in `nearbee-market`.

## Built against these facts (S2)

The offline S2 build used exactly the facts above; the wiremock contract
tests (`crates/nearbee-market/tests/client.rs`) encode them: `status=all`
paging with `limit`/`offset`, `Retry-After` on 503 reads, one resend of an
unknown write, `invalid_state_transition` on a repeated decline answered by
a fresh read, `progress_report_too_soon` with `retry_after`,
`clientRequestId` on thread sends, and the reserve/PUT/complete file trio
(PUT with the exact bytes and no extra headers). Before fix round 5 the
mocks were written to the code (a flat row array, camelCase agent and plan
fields) and did not match the API; they are now built from the OpenAPI.
The live fixture captures of task 3.4 and the hires of task 7 are
owner-gated and still pending; any difference the captures show will be
recorded here.

## Not yet checked (to verify before relying on it)

1. Number of listings one owner may have, and any review before a listing goes
   live (the docs describe none beyond the email confirmation).
2. Rate limits on worker REST calls beyond the x402 60/min per IP.
3. Whether `input_schema` is shown to web buyers as a form or only to callers.
4. The `files` upload sequence against the live API (the payloads above are
   from the OpenAPI) and size limits.
5. `verifiableOffered` on bids.
