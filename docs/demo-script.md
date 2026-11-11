# Demo script (shot list)

The video for the X post (O-8) and the submission's `demoUrl`. The owner
records and posts; agents prepare. Shots in order, each with the exact
command or page, what the viewer sees, and the owner item it needs. A shot
whose owner item is still open is skipped, and the post says nothing about
it (`docs/x-post.md`: every claim backed by `docs/demo-log.md`).

Before recording: `cargo build -p nearbee` once, apb `1.0.0-beta.7` on
Nearbee's own instance (`scripts/apb-instance.sh start`), `nearbee config
check` all `ok`, a terminal with a large font, the market web app signed in
as the owner in a browser.

| # | Shot | Command or page | What the viewer sees | Owner item |
| --- | --- | --- | --- | --- |
| 1 | The walkthrough without any account | `cargo run -p nearbee -- replay` | The banner (scripted, contacts nothing, the signature is synthetic), five scenarios step by step with the hire state and each market write, then `replay: 5 of 5 scenarios ok` | none |
| 2 | One playbook becomes a listing | `nearbee list playbooks/repo-audit/listing.repo-audit-fast.yaml` | The tier check passes as `attested` (script nodes only, one direct and signed model), the installed version, the table of values for the web app and the generated data statement | O-2 (agent id and token of `repo-audit-fast`, done) |
| 3 | The listing in the market | the agent's page on https://market.near.ai | The `repo-audit-fast` agent with its price and the statement in its bio | O-6 (go-ahead to set the listing live) |
| 4 | Nearbee serves | `nearbee serve` in one pane, `nearbee hires` in another | serve polling; the hire arriving, intake, the start reports, the run, the end reports, the submit | O-3 (Cloud credits), O-6 |
| 5 | Another agent hires and pays | `cd examples/x402-hire && node index.mjs repo-audit-fast "Audit https://github.com/itechmeat/nearbee for security problems." --max-usd 0.60` | The 402 challenge, the quoted amount within the ceiling, the signed NEP-366 delegate, the paid call accepted and its task id | O-5 (caller account with USDC), O-6 (the spend) |
| 6 | The deliverable | the hire's deliverable in the market web app | Findings with checked file and line citations, the endpoint attestation disclosure, the `Response signature` section and the data statement | as shot 5 |
| 7 | The receipt checks | the `Check the signature in the NEAR AI Verification Center` link of shot 6 | The Verification Center checking the response signature of the review call | as shot 5 |
| 8 | Agent B on an invoice scan | a hire of `doc-digest-private` from the web app with one invoice PDF or PNG attached; `nearbee serve` running | The digest: summary, entities, checked figures, personal data counts by category, the response signature | O-2 (agent of `doc-digest-private`), O-3, O-6 |
| 9 | Accepted and paid | the hire accepted in the web app, then the owner's balance | The held payment released on acceptance, the earnings in the owner's balance (withdrawal optional) | O-6; O-4 is the prize payout account, not shown |

Notes for the voice-over, all facts from the repository:

1. Nearbee is an adapter: any apb playbook becomes an agent for hire;
   the two agents are examples.
2. The attested listings run no agent: every model call is Nearbee's own
   direct call to a NEAR AI Cloud TEE model, and one call per hire is
   signed by the model TEE (D-026, D-027).
3. Every market run is an apb external-data run, so apb learns nothing
   from buyer data (`docs/privacy.md`).
4. The replay in shot 1 is scripted: it proves the state machine and the
   deliverable path, not the live market; shots 4 to 9 are the live
   proof, recorded in `docs/demo-log.md`.
