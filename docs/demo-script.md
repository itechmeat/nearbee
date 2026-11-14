# Demo script (shot list)

The video for the X post (O-8) and the submission's `demoUrl`. The owner
records, edits and posts; agents prepare. 60 to 90 seconds, cut from
material already recorded: the replay, the paid hires of 2026-11-13 and
2026-11-14 with their deliverables, the market pages, the sandbox and the
`astro-site` result. Every claim on screen or in the voice-over is backed
by `docs/demo-log.md`; a shot without its record is cut, and the post says
nothing about it (`docs/x-post.md`).

| # | Seconds | Shot | Source | What the viewer sees |
| --- | --- | --- | --- | --- |
| 1 | 0 to 8 | The problem | title card over the banner (`brand/poster/banner.png`) | "Your playbooks earn nothing while idle. Nearbee lists them as agents on the NEAR AI Agent Market." |
| 2 | 8 to 18 | No account needed | terminal: `cargo run -p nearbee -- replay` | The banner lines (scripted, contacts nothing, the signature is synthetic), scenarios scrolling, then `replay: 5 of 5 scenarios ok` |
| 3 | 18 to 28 | The agents on the market | the owner's agent pages on https://market.near.ai | `bug-repro-fix`, `vuln-deps-fix` and `repo-audit-fast` with their prices and the data statement in the bio |
| 4 | 28 to 40 | Another agent hires and pays | terminal: `examples/x402-hire` hiring `bug-repro-fix` (hire `d28caae2`), `nearbee serve` in a second pane | The 402 quote of 2.00 USDC, the signed NEP-366 delegate, the paid call accepted; serve taking the hire, the run, the uploads, the submit |
| 5 | 40 to 50 | The deliverable | the hire `d28caae2` in the market web app | The report with the failing test before the fix and passing after, and the files `fix.patch` and `bugfix.json` |
| 6 | 50 to 58 | A second paid hire | the hire `37008197` (`vuln-deps-fix`, 1.00 USDC) in the web app | The report and `patch.diff`; caption "npm audit: 29 vulnerable packages to 5 low, tests still pass" |
| 7 | 58 to 66 | Private AI on NEAR AI Cloud | the `repo-audit-fast` deliverable of hire `dbfbe061` | Findings with checked file and line citations, the `Response signature` section and its Verification Center link |
| 8 | 66 to 74 | The sandbox | terminal: `NEARBEE_APB_SANDBOX=1 scripts/apb-instance.sh status`, then `docker exec nearbee-apb-sbx curl -sS -m 5 https://example.com` | The instance in its container; an outside host does not resolve from inside |
| 9 | 74 to 84 | A website from one brief | the `astro-site` result: the delivered site opened in a browser | The built pages of the site with its brand, copy and generated images |
| 10 | 84 to 90 | Close | title card | The repository link, "MIT", `cargo run -p nearbee -- replay`, `@NEARProtocol #NEARHackathon` |

Shot 9 is the only one whose run is not yet in `docs/demo-log.md`: record
the `astro-site` hire there before posting, or keep the shot without any
claim of a paid hire in the post.

Notes for the voice-over or captions, all facts from the repository:

1. Nearbee is an adapter: any apb playbook becomes an agent for hire; the
   agents shown are the owner's.
2. Buyers pay in USDC on NEAR through the market; another agent pays over
   x402 (shots 4 and 6, `docs/demo-log.md`, 2026-11-14).
3. The attested listing runs no agent: every model call is Nearbee's own
   direct call to a NEAR AI Cloud TEE model, and the response is signed by
   the model TEE (shot 7, D-026, D-027).
4. Every run executes in a container with a read-only system, none of the
   owner's files or keys, and outbound traffic limited to named hosts
   (shot 8, `docs/apb.md`, "Sandboxed instance").
5. The replay in shot 2 is scripted: it proves the state machine and the
   deliverable path, not the live market; shots 4 to 7 are the live proof.
