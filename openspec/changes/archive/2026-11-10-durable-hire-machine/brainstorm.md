# Brainstorm

## Problem

S2 serves one first-time hire at a time and blocks on its run until the run
ends, so while one hire works no other row is polled: a cancellation, an
acceptance, a rework request or a new hire of another listing waits up to a
full SLA. It does not handle rework, redo, disputes, acceptance or cleanup,
does not check a brief before starting, has no cost ceiling and downloads no
attachments. The live hires of 2026-11-10 also showed that the agent token
cannot change the agent (price, description, tags), so `nearbee list` cannot
publish what S2 promised. S3 makes the hire machine durable and complete
enough for the two example agents of S4 and S5.

## Related work

1. `docs/stages.md` S3: the delivery list, the done criteria, and the items
   carried from S2 (3.4 fixtures and the token move, the live part of 5.4,
   7.1 and 7.2, 8.2 and 8.3, 9.1). 7.1 and 7.2 ran live on 2026-11-10
   (`docs/demo-log.md`); 5.4 ran live and found the 403 below.
2. Decisions that bind: D-006 (intake, one-hour rule, ceiling plus wall
   clock), D-011 (no questions, no webhooks), D-012 and D-023 (owner test
   data only until apb releases the external-data flag), D-015 (journal
   intents), D-016 (polling is the source of truth), D-018 (owner-set price,
   optional limits, `protection: off`), D-022 (credentials store; owner
   sign-in deferred).
3. Open questions 10 (live market facts) and 12 (deleting apb run
   artifacts): the second is answered below (no route at 1.0.0-beta.6).
4. Archived changes `foundation-and-runner` and `market-and-first-hire`:
   the journal format, the per-hire state derived from the journal, the
   served-version pinning, the contract test against the OpenAPI.
5. No open pull requests.

Facts measured live on 2026-11-10 that this change builds on:

1. `PATCH /v1/agents/{id}` and `POST /v1/agents/{id}/pricing-plans` answer
   `403 agent_token_scope_forbidden` to the agent token ("may run work, not
   change the agent"); `GET /v1/agents/{id}/pricing-plans` answers 200.
2. apb refuses a budget object with neither a token nor a dollar limit
   (`400 invalid_budget`); it accepts a start with no budget object.
3. A market "Test run" carries `escrowAmount: "0"`, `escrowToken: "USD"`
   and a 600-second SLA; the S2 fixed 10-minute margin expired such hires
   at once.
4. The first run detail right after a start may carry `version: null`.
5. `apb decide` on Nearbee's instance answers
   `{"answered":false,"refused":"no_provider"}`: no decision provider is
   configured.
6. apb 1.0.0-beta.6 has no command or route that deletes a run's
   artifacts. `Runner.stop` sends a drain pause, which lets active work
   continue; `apb stop <run>` aborts active executors.
7. apb's `continued_from` starts a fresh run with lineage: params,
   worktree and budget are not inherited, a predecessor may have one
   successor only, and the version is not pinned to the predecessor's.
8. Plain directories inside the workspace share apb's project workdir
   lock, so two runs there queue instead of running side by side.

## Doubts

Put to the partner: how big the move from a blocking wait to per-round
observation is; whether to split S3 into two pull requests; which reported
cost counts as spent; what to send as a budget with `protection: off`; the
lifecycle mapping against the OpenAPI; what `continued_from` does; the
attachment download contract; every crash point that could duplicate an
external write; leaks of brief text, tokens and capability links.

## Approaches considered

1. Concurrency (author): A1 keep the blocking serial loop; A2 one task per
   active hire bounded by a concurrency limit; A3 a per-round state machine
   that never blocks on a run. Partner: A3 fits the S2 code, which already
   re-derives the hire state after every transition; the change is around
   the run wait, the row filter and the "first decision closes the hire"
   rule, not a rewrite. A3 alone does not bound latency: the market client
   can wait minutes inside one read (retries with `Retry-After`), and
   progress sleeps internally; steps need their own time budgets and must
   return to the scheduler instead of sleeping. Real parallel runs need one
   git worktree per hire (finding 8); without that, `concurrent_runs: 2`
   only queues inside apb and burns SLA.
2. Lifecycle (author): map `accepted`, `cancelled`, `disputed`, rework and
   redo rows onto journal events. Partner: reconcile the market lifecycle
   before deriving the execution state; separate "this execution
   delivered" from "this hire is closed"; identify each reopened execution
   by a durable generation key (the row's SLA deadline plus the delivery
   fields), so repeated polls of one rework row never start two runs;
   `dispute_resolved` is terminal and is not acceptance; keep
   `finalizedAt` for retention.
3. Rework (author): buyer feedback as an untrusted param, `continued_from`
   the previous run. Partner: pass everything again (brief, feedback,
   inputs, worktree, budget, execution id), chain from the latest run, and
   clear the previous `out/` before the rework run, or the old valid
   manifest is delivered again; take the newest buyer message only, never
   Nearbee's own.
4. Cost (author): `budget.usd = ceiling - spent`. Partner: count the
   reported `usage.cost_usd` (apb's budget counts subscription equivalents
   too, so both sides agree) as spent; report cash loss separately as
   billed plus unknown basis; missing or partial usage must not become
   zero; with `protection: off` omit the budget object; derive the ceiling
   from the award's `escrowAmount` when it is above zero, since the
   market price is now set by hand and can differ from the file; refuse a
   protected listing whose ceiling is not positive (price below USD 0.27).
5. Ambiguous apb start (author): the S2 rule, start again after a complete
   empty search. Partner: completeness proves no indexed run exists, not
   that an in-flight start cannot still land later.
6. Intake (author): hard limits, then `apb decide is`, fail open when no
   provider. Partner: distinguish "not configured" from "refused" and from
   an answer; never treat a skipped check as a pass in the journal; pass
   the brief to the subprocess without a shell; the one hour counts from
   the row's `createdAt`; `apb decide` outside a run appends a
   record (digests and metadata, per the review round) to the workspace's
   `.apb/decisions.jsonl`, which joins the deletion inventory as not
   covered.
7. Attachments (author): stream with the agent bearer from the market
   origin, verify sha256, save under `inputs/`. Partner: parse and compare
   scheme, host and port; refuse userinfo, fragments, redirects and other
   origins; enforce the declared and the streamed size; write to a
   temporary file and rename after verification; 403 is quarantine or no
   access, not a retry; 503 carries `Retry-After`; deduplicate by
   attachment id.
8. Journal and replay (partner): a digest cannot rebuild a request; keep
   private snapshots of the run request and of every submission next to
   the journal and journal only their digests; `read_journal` treats every
   read error as "no journal" today, only `NotFound` may mean new.
9. `nearbee list` (author and partner agree): materialise, install,
   record, read the pricing plans and print what the owner must set in the
   web app with a match or mismatch per field; say "installed", not
   "published"; zero market writes.
10. Split (author asked): partner recommends one pull request with
    milestones inside it, since recovery, rework, intake and cleanup are
    coupled and a second full review cycle costs more than it saves.

## Decision

1. One change, one pull request, implemented in milestones (tasks groups).
2. Serve becomes a per-round state machine (A3): each round lists the
   assignments of every agent once, reconciles every journaled hire with
   its row first (lifecycle), then advances each open execution by at most
   one step, and never waits for a run to finish. Steps that the market or
   apb ask to wait (progress too soon, attachment still scanned) record
   when they are due and return. The market client's read retries get a
   total time budget per call.
3. At most one active run at a time across all agents in S3. Parallel runs
   need a git worktree per hire and a check that the profiles and the
   OpenCode provider config are found there; that is deferred to S4 and
   written down as a non-goal. `limits.concurrent_runs` is not added yet.
4. Each hire journal holds generations: generation 1 is the first-time
   work, each rework or redo opens the next one, keyed by the row's
   `slaDeadlineAt` and delivery fields, so one market reopening gives one
   generation and one run. A generation ends delivered or declined; the
   hire closes on `accepted`, `cancelled` or `dispute_resolved`.
5. Rework and redo start a continued run (`continued_from` the latest run
   of the hire) with the original brief, the latest message as the
   untrusted `feedback` param when its sender side is the buyer (its full
   body from the thread when the preview is truncated, matched by
   `createdAt`; a rework without reliable buyer feedback is declined
   `failed`; empty for a redo), the same inputs, a new
   `execution_id`, the remaining budget, and a cleared `out/` (the previous
   generation's `out/` is moved into the generation's private snapshot
   first).
6. Cost: spent is the sum over the hire's runs of the reported
   `usage.cost_usd` at the run's end; a run whose cost is missing or
   incomplete (fewer attempts with a cost than finished attempts) counts
   as having spent the whole budget it was given. Ceiling = `amount * 0.95 - 0.25` USD, where
   amount is the row's `escrowAmount` when above zero and the listing's
   price for a zero-escrow owner test run. A protected listing with a
   ceiling not above zero is refused by `nearbee list`. A rework or redo
   whose remaining budget is not above zero is declined `failed`.
   `protection: off` omits the budget object and skips the scope check;
   the wall-clock stop, attachment checks and shutdown still apply. A run
   paused by its budget is stopped and declined `failed` at once.
7. Daily loss: the cost of every execution that ended without a delivery
   (declined, stopped, cancelled) counts towards the UTC day it ended;
   past `limits.daily_loss_usd` (default 20) new first-time hires are
   declined `rejected` with a "not taking work right now" reason, and the
   operator is told once.
8. Stopping a run means aborting it (`apb stop <run>` on Nearbee's
   instance) and confirming a terminal status before the hire moves on;
   an unconfirmed stop stays open and is retried next round.
9. Ambiguous apb start: adopt the single run the `execution_id` search
   finds; more than one is an operator case; an empty search, complete or
   not, never leads to a second start for that generation (changed in the
   review round: completeness does not prove a lost start will not land);
   the generation is declined `failed` at its deadline, the start stays
   open and keeps the run slot, and every round searches the hire's
   `hire_id` and aborts any run that is not the adopted one, closed hires
   included.
10. Intake before start: hard limits from the listing file (brief bytes,
    attachment count, attachment total bytes, file extensions), then the
    scope check through `apb decide is` (skipped with an operator note
    when apb answers `no_provider`, recorded as skipped, never as passed),
    then the cost admission. A decline before start is `rejected`; the
    journal records whether it fell inside the first hour from
    `createdAt`. A row already started by the owner is past intake and
    goes to the run.
11. Attachments of the job and of the newest rework message are
    downloaded into `<worktree>/inputs/` as described in 7 above and passed
    as the `inputs` param, a JSON array of relative paths.
12. Private snapshots under `<state_dir>/hires/<safe-id>/gen-<n>/` (0700):
    the start request body and each submission's Markdown; the journal
    keeps only digests and sizes. `read_journal` reports every error other
    than `NotFound`.
13. `nearbee list` makes no market writes; the client methods for agent
    patch and plan creation stay for the deferred owner sign-in (D-022)
    and are not called with an agent token.
14. A lock file `<state_dir>/serve.lock` held for the life of `serve` and
    taken by `cleanup`; a second holder exits with code 4.
15. `nearbee hires` (read only) and `nearbee cleanup` (with `--dry-run`):
    cleanup removes adapter-owned paths only, reports apb run artifacts and
    `.apb/decisions.jsonl` entries as not covered, journals `deleted` with
    the inventory; disputed hires are never cleaned.
16. Operator notices (O-7): a log line plus a persisted attention code per
    hire shown by `nearbee hires`; push delivery is not built.
17. Output file uploads stay out: a manifest that lists output files is
    declined `failed` with an operator reason until S4 adds them.

## Rejected

1. A1 serial blocking loop: one long run hides every other hire's
   lifecycle for up to a full SLA.
2. A2 a task per hire: concurrency without parallel apb runs buys nothing
   and adds shared counters.
3. Splitting S3 into two pull requests: coupled work and a second review
   cycle.
4. Counting only billed plus unknown-basis cost as spent: apb's own budget
   counts subscription equivalents, so the two would disagree.
5. A huge fixed budget for `protection: off`: apb accepts no budget, which
   says what is meant.
6. Feedback as the run instruction: breaks rule 3 (a buyer message is
   data).
7. A brief parser that maps prose onto playbook params: deferred, only
   adapter-owned params (`brief`, `feedback`, `inputs`) in S3.
8. A buyer start message with the plan and the deadline: an extra write
   for little value; A2A hires have no thread.
9. Push notifications through apb notify: needs a run and an owner
   channel; the attention code covers S3.

## Open questions

None for the owner. Q-1 (retention 7 days) stays the default.

## Process

Partner: Codex (OpenAI family), read-only on the repository and on apb's
source at the installed tag, one round with 26 numbered findings; all were
taken, cut to scope or rejected above with a reason. The author is a
Claude model; the implementer will be a GLM model, so neither the partner
nor the artifact reviewer shares its family.
