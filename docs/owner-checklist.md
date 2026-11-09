# Owner checklist: accounts, keys and go-aheads

Things only the owner can provide or decide. None of them is needed to start
coding; each becomes necessary at a specific step of `docs/plan.md`. **The agent
working on that step asks the owner for the item at that moment**, in one
message, with the exact instructions below, and does not ask earlier. Secrets go
into `.env` on the development machine, never into the repository.

| # | Item | Needed for | Ask at plan item | Status |
| --- | --- | --- | --- | --- |
| O-1 | **Market builder account** at https://market.near.ai with a confirmed email (the signup confirmation link; signing in with Google counts). Optional: link GitHub and X under Building → Profile → Get verified for the "Social media verified" mark. | Publishing agents, minting agent tokens, withdrawing earnings later. | Day 1, item 7 (first draft listing). | done (2026-11-09) |
| O-2 | **One draft agent per example variant**, created in the web app (Building → Your agents → New agent: name, handle, tagline; Hosting: Self-hosted, webhook empty), with an `aat_…` token from "Create token" (shown once). Variants: `repo-audit-fast`, `repo-audit-premium`, `doc-digest-private`. The token is handed to `nearbee agent add <handle> --agent-id <uuid>` once (D-022): a draft agent answers 404 to every lookup its token can make (measured 2026-11-10), so for each draft the owner also gives the agent id, copied from the agent's settings page in the web app. | `nearbee list` needs the agent id and token; `nearbee serve` polls with the token. | Day 1, item 7 (the first one); Day 3, item 15 and Day 4, item 16 (the others). | first variant `repo-audit-fast` done (2026-11-09), token still in `.env`: the move to `nearbee agent add --agent-id` (S2 task 3.4) needs a live market call and stays open in the sandbox; the agent id of the draft is not known to the adapter yet and comes from the owner with the token; the other two variants open |
| O-3 | **NEAR AI Cloud API key** from https://cloud.near.ai (USD 10 of credits via https://nearbuilders.org/join), plus the spend the owner allows for development and the demo. | TEE model profiles (`fast`, `private` variants), the signed digest step. | Day 1, item 4 (Cloud path). | key done (2026-11-09), in `NEARAI_API_KEY`; balance empty, the owner tops up USD 10 later and that is the spend limit |
| O-4 | **NEAR mainnet account for the prize payout** (named `name.near` or implicit). Any wallet; the owner creates it, never an agent. | The `nearAddress` field of the hackathon submission. | Day 5, item 20 (submission payload). | open |
| O-5 | **Second NEAR mainnet account holding 5 to 10 USDC** (contract `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`) with its private key in `.env` (`NEAR_CALLER_ACCOUNT_ID`, `NEAR_CALLER_PRIVATE_KEY`). It plays the "other agent" that hires ours and pays with x402; it must not be the market owner's identity (the market refuses paying your own agent). | The agent-to-agent demo and `examples/` caller. | S2, plan item 6 (signature check, no spend) and S5, plan item 17 (the paid hire). | open |
| O-6 | **Go-ahead for every spend**: Cloud credits beyond development noise, the paid x402 hire, setting listings live. | Items 15, 16, 17. | At each of those items. | per event |
| O-7 | **Owner notify channel** for declines and failures (Telegram through apb notify assumed). | `serve` error reporting. | Day 2, item 12. | open |
| O-8 | **Demo video and X post** (built at NEAR HACKATHON, tags `@NEARProtocol` `#NEARHackathon`, video attached). The agent drafts the text; the owner records and posts. | Submission `demoUrl`. | Day 5, item 20. | open |
| O-9 | **Submission confirmation**: the owner reads the full JSON payload and says yes; it is sent once. | `docs/hackathon.md`. | Day 5, item 20. | open |
| O-10 | **Go-ahead to create the public GitHub repository** and push `main` (no remote until then, owner 2026-11-09). Must be public with `LICENSE` by 2026-11-14T23:59:00Z. | `repoUrl` of the submission; judges' clean checkout. | Day 4, item 18 (clean-checkout run), at the latest Day 5. | done (2026-11-09): https://github.com/itechmeat/nearbee, public |
| O-11 | **Worktree-only file access for the market profiles**: the apb profiles behind the market listings (`fast`, `private` variants) must restrict connector and executor file access to the run's worktree. This is an apb-side profile setting, not adapter code; the owner checks it in the apb instance (and in the playbook sources) before a listing goes live. | Privacy tier enforcement behind the listings (rule 7), the F1 secret-exfiltration review. | Day 3, item 15 (first live listing). | open |

When an item is provided, the agent sets its Status to `done (date)` here and
notes where the value lives (which `.env` variable), never the value itself.

Ask for an item only when its plan step is next. If several items fall on the
same day (Day 1: O-1, O-2, O-3, O-5), ask for them together in one message.
