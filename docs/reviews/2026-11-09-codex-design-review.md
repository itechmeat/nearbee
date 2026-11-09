<!-- Codex (gpt-6.1-sol, read-only) design review of the project record at commit 75335b1, 2026-11-09. Findings verified against the sources by the orchestrator; corrections landed in the next commit. Local absolute paths in links are the reviewer's machine paths. -->

# Nearbee project-record review

**The adapter is feasible, but the record needs corrections before implementation.** The main blockers are SKU execution, recovery after interrupted requests, and privacy claims stronger than the proposed evidence.

Static review against the local market references, apb routes/docs, and public Cloud/legal pages. No files changed. No output path was supplied, and this session permits no filesystem writes, so the report is returned here.

Project paths below are relative to the repository root. **APB** means the agentic-playbooks repository.

## 1. Factual errors

1. **Hire classification will miss work and dispute redos.**  
   [architecture.md:69](docs/architecture.md:69), [market.md:80](docs/market.md:80). `startedAt: null` does not identify all new work: the owner or webhook may already have stamped start. A redo also differs from request-changes: it clears `submittedAt` while preserving `deliverableUrl`. Source: vendored `flows/worker-lifecycle.md`, classification table: **“the resolver sent it back and cleared `submittedAt`.”** Classify using delivery fields, then consult the local journal to determine whether Nearbee already began work.

2. **REST cannot execute the listing’s promised version/profile overrides.**  
   [architecture.md:50](docs/architecture.md:50), [apb.md:115](docs/apb.md:115), [open-questions.md:28](docs/open-questions.md:28). APB `routes/playbooks.rs`, `RunBody`, has neither `version` nor `overrides`. The handler constructs `PlaybookRef { version: None }` and runs the gate-selected version. Merely supplying separate profile files does not change node bindings. Use separate immutable playbook IDs or workspaces per SKU; otherwise add an apb API capability.

3. **The USD budget neither guarantees a ceiling nor supports the documented cold start unchanged.**  
   [pricing.md:57](docs/pricing.md:57), [architecture.md:75](docs/architecture.md:75). APB `HOWTO-authoring.md`, “budget”: **“It is a predictive stop: before each agent node and each `playbook` node…”** An admitted node can overspend. With no cost history, default `on_no_estimate: ask` makes HTTP start return `409 budget_needs_decision`; Nearbee currently refuses that gate. Define an explicit bootstrap policy and additional per-call/time limits.

4. **Run-level endpoint evidence does not prove this job’s response ran in a model TEE.**  
   [privacy.md:13](docs/privacy.md:13), [privacy.md:51](docs/privacy.md:51), [README.md:44](README.md:44). The table and README promise response signatures, while v1 defers collecting them. Cloud states: **“Only `provider_tee` binds a response to a model-serving TEE.”** Verification needs exact request/response bytes and matching verified preflight evidence; streamed responses commonly receive gateway signatures. Fetching reports at run start/end cannot supply that binding. Narrow the claim or implement the complete path. [Response-signature contract](https://docs.near.ai/cloud/verification/cloud-api/response-signatures).

5. **The document showcase assumes specialized models work like coding-agent chat models.**  
   [showcase-agents.md:24](docs/showcase-agents.md:24), [apb.md:124](docs/apb.md:124). Cloud says privacy-filter **“is not a chat model”** and accepts text through dedicated endpoints. It cannot run first on an invoice image without preceding extraction. The saved Qwen3-VL entry also lacks `tools`; its supported input is image content parts, not arbitrary PDF/DOCX files. Specify local document conversion → vision/extraction → PII classification → digest, with page/pixel limits. [Specialized-endpoint contract](https://docs.near.ai/cloud/guides/specialized-endpoints).

6. **Jev contradicts the proposed catalog-based tier validator.**  
   [near-ai-cloud.md:22](docs/near-ai-cloud.md:22) places `typesafe/jev-1.13` under `owned_by: nearai`; the saved catalog entry at `docs/reference/near-ai-cloud-models.json:903` says **`"owned_by": "typesafe"`**. Thus the validator in [privacy.md:18](docs/privacy.md:18) would reject it. Resolve its actual processing/evidence policy separately; a catalog ownership field alone is insufficient evidence.

7. **apb stats/estimate do not provide the promised p90 figures.**  
   [pricing.md:20](docs/pricing.md:20). APB `STATS.md`, “Figures”: **“A range is p25 to p75 from 4 samples.”** Estimates expose medians/ranges; stats expose aggregates such as median/max duration. Nearbee must calculate p90 from individual observations or use explicitly named conservative estimates. Do not describe the existing response as p90.

8. **The legal summary changes retention language and omits liability exceptions.**  
   [legal.md:27](docs/legal.md:27), [legal.md:41](docs/legal.md:41). Job Terms §7.3 says **“delete or stop using”** on completion, with stated exceptions; it does not prescribe retention through acceptance/dispute periods. Section 11.5 also excludes the §8 non-infringement obligation/indemnity from the job-price cap. Separately, §3.2 says held funds are **“not held in escrow”**: distinguish the API’s escrow terminology from the legal arrangement. [Current Job Terms](https://market.near.ai/job-terms).

## 2. Design risks

9. **Profiles plus connector allowlists cannot enforce the advertised privacy boundary.**  
   [privacy.md:16](docs/privacy.md:16), [architecture.md:35](docs/architecture.md:35). APB `PROFILES.md` explicitly says **“`environment` is not a sandbox.”** Agents/scripts can use shell HTTP, local credentials, project configuration and other tools. Decision providers can also receive prompts/outputs independently of executor profiles (`DECISIONS.md`, “What is sent”). Include those paths, sub-playbooks, routed tiers, hooks and provider configuration in validation. Use a dedicated execution environment and controlled egress for an enforced claim.

10. **“Idempotent and logged” lacks a recovery protocol.**  
    [architecture.md:37](docs/architecture.md:37), AGENTS.md engineering rule 5. APB start returns a server-generated `run_id` and accepts no caller request ID. A timeout after creation leaves Nearbee unsure whether retrying will create another run. Specify durable intent records, exclusive hire ownership, recovery lookup, and a policy for ambiguous starts. For market messages, use `clientRequestId`; for submit retries, preserve identical Markdown and file sets. Source: vendored `flows/messaging.md`, “Write”, and `worker-lifecycle.md`, “Resubmitting”.

11. **The default poll cannot support the complete lifecycle or deletion timer.**  
    [architecture.md:65](docs/architecture.md:65), [architecture.md:90](docs/architecture.md:90). The worker source says the default queue is **“`in_progress` only”**; submitted work disappears. Poll relevant non-active statuses with pagination, preserve submitted-thread support, and reconcile cancellation/dispute states. Never infer acceptance solely from elapsed `autoAcceptAt`. Stop local execution after refunds or cancellation. Admission must use the live `slaDeadlineAt`: its OpenAPI description starts the clock at award, before intake and queue waiting.

12. **Synchronous webhook intake conflicts with delivery semantics and credentials.**  
    [architecture.md:65](docs/architecture.md:65), [architecture.md:55](docs/architecture.md:55). Vendored `reference/webhooks.md` says **“Answer `2xx` quickly”** and disables delivery after ten consecutive failures. Holding a response for downloads/model decisions—or an unanswered intake question—creates retries and outage risk. It also says an `aat_` token cannot configure webhooks (`403 agent_token_scope_forbidden`), although another vendored quickstart conflicts with this. Use polling for v1; require owner-side webhook setup if later enabled.

13. **Question relay needs request correlation and an A2A-specific policy.**  
    [apb.md:79](docs/apb.md:79), [architecture.md:78](docs/architecture.md:78). APB `routes/runs.rs` exposes `request`, the question/review journal sequence; it rejects stale requests. Persist `(run, node, request.seq)` and buyer-message IDs, and relay human answers verbatim. The x402 caller skill states **“Multi-turn (`message.taskId`) is not supported.”** A standard A2A caller therefore cannot complete the proposed clarification loop. Make A2A-compatible playbooks self-contained, with validated defaults or early rejection.

14. **Attachment and deliverable handling needs an explicit safe contract.**  
    [architecture.md:69](docs/architecture.md:69), [apb.md:72](docs/apb.md:72). Initial attachments are insufficient: message files and A2A non-text parts have separate recovery paths (`flows/messaging.md`, “Read”). Add safe filenames, origin-restricted authenticated downloads, streaming limits, parser/decompression limits and output-path containment. Enforce 256 KiB in UTF-8 bytes after adding evidence. The submit contract also limits file sets and requires kept files to be relisted. **Fail closed on missing designated output**: a supervisor/run report can expose internal material and is not a substitute for the purchased result.

## 3. Scope vs time

15. **The current plan is too broad for five implementation days—engineering estimate.**  
    [plan.md:15](docs/plan.md:15) puts two clients, publishing and end-to-end delivery on day one, then adds interaction, rework, cleanup, multimodal parsing, privacy evidence and payment signing. The highest-risk Cloud/payment checks arrive too late.

    Cut first: blind mode, showcase C, webhooks, generated pricing, buyer questions and owner review gates. Keep two narrowly scoped showcases on one worker loop.

    | Date | Recommended exit condition |
    |---|---|
    | Nov 10 | Verify one exact Cloud call, apb start/result, and x402 signing path; complete one draft hire |
    | Nov 11 | Durable journal, recovery, cancellation, terminal-state polling and capped intake |
    | Nov 12 | First bounded showcase, explicit result contract and honest verified evidence |
    | Nov 13 | Second showcase, authorized paid demo and clean-checkout walkthrough |
    | Nov 14 | Defect buffer, recording and submission preparation |

    These are implementation estimates, not measured durations. Native cold-start and interrupted-request checks belong before polishing listings.

## 4. Hackathon fit

16. **The showcase choices need a sharper buyer outcome—product judgment.**  
    [showcase-agents.md:7](docs/showcase-agents.md:7). Category size demonstrates competing supply, not demand. A general public-repository audit provides a weak privacy demonstration. Narrow A to a concrete NEAR builder task, such as checking a release’s configuration and documentation for deployment inconsistencies. Narrow B to invoice extraction with verified totals, source references and machine-readable JSON useful in a DAO’s payment-review workflow. Drop C: selling apb authoring mainly demonstrates the dependency.

17. **The proposed demo timing and payout claim are unreliable.**  
    [showcase-agents.md:47](docs/showcase-agents.md:47). A 30–60 second poll interval plus a repository audit leaves little basis for “result in under a minute.” The worker contract says **“The payout is a separate step”** and warns not to book it on acceptance; x402 treasury payouts usually take minutes. Show request → delivery → payment settlement, then separately show confirmed payout. The chosen tracks fit without inventing an Intents requirement: hackathon Q-011 says integrations are hints, not mandatory.

## 5. Pricing and privacy

18. **The economic model omits failed hires, rework and shared capacity.**  
    [pricing.md:20](docs/pricing.md:20), [architecture.md:86](docs/architecture.md:86). Each rework becomes a fresh run, so resetting the cost ceiling per run defeats a per-hire ceiling. Reserve for rework, intake, failures and operational costs; track cumulative spend across the entire hire. Add concurrent-run and daily-loss limits. APB history may have incomplete cost reporting, and subscription usage is not automatically equivalent to marginal API cost (`STATS.md`, “What is measured”).

19. **Price and SKU rules are underspecified.**  
    [pricing.md:33](docs/pricing.md:33), [pricing.md:68](docs/pricing.md:68). The worked example rounds upward, while the rule merely says “rounded”; `min_margin` has no unit/default. Define decimal arithmetic and ceiling rounding. Fast already uses TEE models, so the private premium needs an explicit additional service. Section count and output format can affect cost despite being called cost-neutral. Separate model quality, evidence and input-size promises in each SKU.

20. **Deletion and subprocessor statements need tighter boundaries.**  
    [privacy.md:34](docs/privacy.md:34), [privacy.md:66](docs/privacy.md:66). Inventory apb snapshots/journals, CLI transcripts, caches, temporary conversions and backups—not just worktrees. A dedicated project does not itself disable learning features; document exact settings. The market also processes thread messages and files. Its worker source says **“Once a job uses a file it cannot be deleted”** and public file links work for anyone holding them. Promise local deletion precisely; disclose platform-held copies separately.

21. **Blind mode conflicts with the execution and intake designs.**  
    [privacy.md:14](docs/privacy.md:14), [decisions.md:15](docs/decisions.md:15). A coding-agent CLI cannot interpret encrypted input without defeating blindness. Relay scripts need a separate encrypted-call protocol, while semantic intake cannot inspect the brief. Define ciphertext size/token limits, key freshness, retries and encrypted delivery. Cloud E2EE protects supported Chat fields; it does not automatically encrypt a whole playbook. This warrants a separate design after the hackathon. [E2EE contract](https://docs.near.ai/cloud/guides/e2ee-chat-completions).

## 6. Ideas

Estimates below overlap with the revised plan.

22. **Materialize immutable SKU playbooks — 0.25–0.5 day; no apb change required.**  
    Addresses finding 2 and [architecture.md:50](docs/architecture.md:50). Generate separate registered playbook IDs with fixed profile bindings, explicit workspace identity and digests. Check the executed run’s version against the resolved listing. This makes the REST limitation manageable without racing shared profile edits.

23. **Explicitly reconsider D-008 for a narrow signed-inference showcase — 0.75–1.5 days; no apb change required.**  
    [decisions.md:15](docs/decisions.md:15). A trusted apb script node can perform one bounded Cloud call and save its verification receipt. This gives control over exact bytes, canonical IDs and signer classification that a general CLI currently hides. Keep the adapter behind `Runner`; record the design revision openly.

24. **Require a deterministic result manifest — 0.25–0.5 day; no apb change required.**  
    [apb.md:72](docs/apb.md:72). Require `out/result.json` naming the Markdown, files and evidence, with hashes and sizes. Validate containment and completeness before submit. APB already exposes `RunDetail.outputs`; `outputs.files` captures artifacts but only warns when missing. Nearbee supplies the stricter delivery gate.

25. **Ship a replayable judge walkthrough — 0.5–1 day; no apb change required.**  
    [plan.md:53](docs/plan.md:53). Reuse the same core with recorded market responses and a fake runner to demonstrate success, oversized rejection, redo, restart and cancellation without credentials. Keep a separate live walkthrough. This improves reproducibility and directly exercises the failures identified above.

## 7. Questions for the owner

26. **Is Bun + TypeScript approved?**  
    [decisions.md:10](docs/decisions.md:10) explicitly leaves D-003 unconfirmed before first code.

27. **Which privacy promise is the launch contract?**  
    [privacy.md:13](docs/privacy.md:13). Choose endpoint-attestation disclosure or verified job-response evidence, and decide whether to approve the narrow script path in finding 23. This blocks the privacy implementation and listing wording.

28. **What exactly does the fixed price include?**  
    [showcase-agents.md:24](docs/showcase-agents.md:24), [pricing.md:45](docs/pricing.md:45). Confirm supported input formats, page/repository limits, included rework scope, retention policy and maximum owner loss. File bytes alone do not define a bounded service.

29. **Are the owner account, two demo identities and spending authorization ready?**  
    [open-questions.md:9](docs/open-questions.md:9). Confirm credentials through local secret configuration and specify the permitted Cloud/mainnet demo spend. This blocks paid integration validation, though fixture-based core implementation can proceed.