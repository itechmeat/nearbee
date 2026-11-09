# Brainstorm

## Problem

Stage S2 (`docs/stages.md`): Nearbee must talk to the NEAR AI Agent Market
for the first time. It needs a market client behind the `Market` seam, its
own store of agent tokens, a way to publish one draft listing from a
materialised playbook, a `serve` loop that takes a hire, runs a playbook and
submits the result, the first end-to-end hire by the owner, and a demo caller
that pays an agent with x402. It must not double-start a billed apb run or
double-submit across a restart, even though the durable hire machine is S3.

## Related work

1. S1 (merged): `Market` trait stub, journal types and layout, manifest
   validator, `ApbRunner` with lookup by `execution_id`, transport rules
   (no redirects, no proxies, no transport retries).
2. Decisions D-003 (Rust, `near-api-js` fallback for the demo caller), D-009,
   D-013, D-015, D-016, D-022 (Nearbee-owned tokens), D-023 (external data).
3. `docs/apb.md`: market playbooks must declare `trial_candidates: off`.
4. Live check by the author on 2026-11-09: a draft agent takes no x402 calls
   (`-32004`, card 404), so the no-spend x402 proof needs the agent live.

## Doubts

Endpoints and payloads, hire classification, owner hiring their own draft,
rate limits, x402 on a draft, idempotency of submit and decline, the 256 KiB
limit and file upload, scope for one day, NEP-366 crates.

## Approaches considered

1. **Client** (author and partner): one `NearAgentMarket` with the S1
   transport rules; reads retry; writes retry only where the market makes
   them idempotent (`start`, identical resubmit, `clientRequestId` on
   messages); `decline` retried once with `400 invalid_state_transition`
   counted as landed; `progress` only after `409` with `retry_after`.
   Paging stops on a short page (`MyAssignmentsResponse` has no total).
   Adopted.
2. **Fixtures** (partner): every call can be captured with the owner's draft
   token: agent read, patch, pricing plans and the file trio need no hire;
   start, progress, messages, submit need the owner's web hire; decline
   `rejected` needs a second hire. Capability links (`/files/{token}`),
   presigned upload URLs and tokens are scrubbed. Adopted.
3. **Credentials** (partner refined the author): JSON keyed by handle with
   `{agent_id, token}`, mode 0600 checked on every read (refuse wider),
   atomic write. There is no `GET /v1/agents/me`: `agent add` resolves the
   id from the public `GET /v1/agents/{handle}` and proves the token with
   `GET /v1/agents/me/assignments?limit=1`, saying which of the two failed.
   Adopted.
4. **Listing** (both): a minimal listing YAML (handle, source playbook,
   price, SLA, category, tags, description, bio); publish = materialise with
   `trial_candidates: off` and fixed profile names, patch the agent, create
   the per-call pricing plan and make it default, write the resolved listing
   under `.nearbee/listings/`. Nearbee never changes `listing_status`: going
   live stays the owner's action (hackathon rule). Adopted.
5. **Serve state** (author asked: memory only vs intents; partner: write
   intents now). Adopted the partner's version: S2 adds the journal
   **writer** (append, fsync) and records `received`, the start intent and
   result, the run intent and the observed run id, the submit intent and
   result. A restart finds a run by `execution_id` before starting another.
   No lock file, recovery table, cancellation or refunds (S3).
6. **x402** (partner): the `near-api-js` example of the caller skill, not
   Rust: Rust would need RPC block height, Borsh and signing hand-wired in a
   day. The signature is also verified locally (receiver, `ft_transfer`
   arguments, block window) and the result recorded, so the proof of the
   signing code does not hinge on the relayer. Adopted; D-003 exercised.
7. **No-spend proof** (author's live check decided it): the draft does not
   take calls, so the owner sets `repo-audit-fast` live at a token price for
   the test, the demo caller account pays, `serve` declines `rejected`
   before any submit, the hold is released; the owner may set it back to
   draft afterwards. Needs the owner's go-ahead (O-6).

## Decision

As in the approaches above. S2 handles first-time hires only: a row that is
rework, a dispute redo, or a submitted row with a new message is logged and
left alone (S3). `cargo deny` goes first. File upload exists in the client
and its fixtures, not in `serve`. `escrowToken` may be `USD` or `USDC`;
nothing in S2 assumes either. A REST 401 stops polling that agent and tells
the owner; a `cancelReason` on a row stops local work for it.

## Rejected

1. Memory-only serve state: a crash would start a second billed run.
2. Rust NEP-366 signing in S2: too much hand wiring for one day.
3. A general listing editor or `list --live`: one listing, owner goes live.
4. Handling rework, redo and questions: S3.
5. TOML credentials: JSON matches the other machine-written state.

## Open questions

None for the owner beyond the go-ahead items already in the stage
(O-5, O-6 for the live no-spend test).

## Process

Partner: Golem (GLM family, different from the author), read-only by brief,
one round; Codex, the first choice, was out of quota. The partner wrote its
report into this file against the brief; the author rewrote it into this
structure and kept the full report outside the repository.
