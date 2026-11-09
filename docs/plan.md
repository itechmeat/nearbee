# Plan to the deadline (2026-11-14T23:59:00Z)

Today is 2026-11-09. Five working days. The riskiest unknowns (a Cloud model
call from apb, the apb start and result path, the x402 signature) come first;
polish comes last. Every day ends with the README's "Try it" steps still true
for what exists. Items are numbered for reference.

## Cut from v1 (decided 2026-11-09 after the Codex review)

Blind tier, example agent C, webhooks, buyer questions during a run, owner
review gates in market playbooks, auto-pricing (owner-set prices in v1). Two
example agents on one worker loop.

## Day 0, 2026-11-09 (Sunday): design

1. This documentation set, reviewed by Codex; corrections landed
   (`docs/reviews/2026-11-09-codex-design-review.md`).
2. Owner decisions recorded (D-003 Rust, D-008 signed digest step, D-018
   owner-set prices, D-019 owner checklist). Owner-provided items are requested
   at the step that needs them: `docs/owner-checklist.md` (Day 1 asks for O-1,
   O-2, O-3 and O-5 in one message at the start of the day).

## Day 1, 2026-11-10: prove the three risky paths, one draft hire

3. Cargo workspace (`crates/nearbee-core`, `nearbee-market`, `nearbee-apb`,
   `nearbee`), config loading, `.env.example`, journal format.
4. **Cloud path**: an apb profile on NEAR AI Cloud through OpenCode's custom
   provider; one trivial playbook run on `Qwen/Qwen3.8-27B` and on
   `DeepSeek-V4.1-Flash-1017`; record what works in `docs/apb.md`.
5. **apb path**: `runner/` against the local dashboard: list, estimate, start
   with `hire_id`, poll `RunDetail`, read `outputs.files`, find a run by
   `hire_id`; confirm the manifest convention end to end.
6. **x402 path**: `examples/x402-hire` signs a NEP-366 delegate for a 402 row
   with `near-primitives` or `near-api` (against any live agent, cancelled
   before delivery or at the smallest price) to prove key type and relayer
   acceptance; if the Rust path is awkward, a `near-api-js` script instead
   (D-003). No spend without the owner's go-ahead (O-6).
7. `market/` client (assignments in all statuses, start, messages with
   `clientRequestId`, progress, files, submit, decline, agent patch, pricing
   plans) with recorded responses; `nearbee list` publishes one draft listing;
   the owner hires the draft from the web; `serve` takes it, runs a trivial
   playbook, submits. First end-to-end hire.

## Day 2, 2026-11-11: durable hire machine

8. Journal with intents, recovery table, lock file, cancellation and refund
   handling, polling of terminal states, dispute freeze and redo.
9. Intake: optional hard limits, scope check through `apb decide`, decline
   texts, the one-hour rule; per-hire cumulative ceiling and run budget with
   the wall-clock stop; global limits; `protection: off`.
10. Attachments: authenticated streamed download, sha256, safe names,
    worktree layout, params mapping; manifest validation; 256 KiB check.
11. Rework and redo as continued runs with the remaining budget.
12. `nearbee hires`, `nearbee cleanup` with the deletion inventory.

## Day 3, 2026-11-12: example agent A, tiers, statements

13. Showcase A `repo-audit` playbook: `premium` (owner's subscription) and
    `fast` (Cloud TEE) variants materialised; both run on a real public
    repository; manifest with `findings.json`.
14. Tier validator; data statement generation; evidence block (run-level
    attestation disclosure); the signed script step for the digest (D-008).
15. Publish A's variants live under the owner's account (owner confirms).

## Day 4, 2026-11-13: example agent B, paid demo, judges' path

16. Showcase B `doc-digest-private`: local conversion, Qwen3-VL per page,
    `/v1/privacy/classify`, digest, `digest.json`; limits; live listing.
17. Paid agent-to-agent hire of `repo-audit-fast` from the demo caller account
    (owner's go-ahead for the spend); record task id, settlement and the later
    payout in `docs/demo-log.md`.
18. README "Try it" exact on a clean checkout; `cargo test` green; `nearbee
    replay` fixture walkthrough (success, oversized decline, redo, restart,
    cancellation) that needs no credentials.

## Day 5, 2026-11-14: buffer and submission

19. Fix what the clean-checkout run found; keep listings live; final docs pass.
20. Owner records the video and posts on X; owner confirms the payload; the
    submission is sent once (`docs/hackathon.md`).

## Risks and the fallback for each

| Risk | Fallback |
| --- | --- |
| OpenCode custom provider does not work with Cloud reasoning models inside apb | `Qwen/Qwen3.8-27B` without reasoning; or the trusted `script` node calling the OpenAI-compatible API directly for the model steps of example agent B (which also gives signed evidence) |
| Market publishing limits block a second or third listing | run the example agents as drafts under one owner account; the SKU story is shown with drafts |
| x402 payment fails on mainnet (relayer, key type, balance) | show an agent-to-agent hire over MCP from a second account; keep the x402 attempt in the video with its error explained |
| apb needs a change (result contract, external-data flag, version on start) | Nearbee-side conventions (manifest, materialised playbooks, dedicated project); open the apb issue and link the diff in the README if a change lands in the window |
| Owner's time for the video | a screen recording of the terminal and the market page (Q-012) |
