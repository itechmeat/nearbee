# NEAR Hackathon: rules that bind this project

Source: https://nearhackathon.com/HACKATHON.md (copy of 2026-11-09 in
`docs/reference/HACKATHON.md`). Re-read the live file before submitting.

## Dates (UTC)

| Event | When |
| --- | --- |
| Code counts from | 2026-10-19 (announcement) |
| Development window | 2026-11-01T00:00:00Z to 2026-11-14T23:59:00Z |
| Submission deadline | 2026-11-14T23:59:00Z |
| Judging | from 2026-11-16 |
| Results | by 2026-11-18T23:59:00Z |
| Keep the demo up | until 2026-11-18, ideally 2026-11-25 |

## What must be true at submission

1. Public repository with a `LICENSE` file (MIT chosen).
2. `README.md` readable by Claude Code and Codex, which screen every project
   (reproducibility check, feature verification, preliminary score; humans
   decide). Format: what it is, the problem, and "do X, see Y" steps.
3. Baseline disclosure (Q-003, Q-005): `apb` is the owner's pre-hackathon library
   and counts as baseline. Nearbee is a new repository created inside the window,
   so no tags or diff links are needed here; the README names apb as baseline.
   If apb itself needs changes for Nearbee, they land in apb's own repository and
   are listed in the README as window work with a link to the diff.
4. A public demo post on X by the owner: built at NEAR HACKATHON, tags
   `@NEARProtocol` and `#NEARHackathon` (optional `@NearLegion`,
   `@NearBuilders`), with a video. A local-only demo is acceptable with a video
   (Q-012); screenshots alone are not.
5. One submission per project, sent only after the owner confirms the exact
   payload. Fields: `requestId` (UUID, generated once), `projectName`, `leadName`,
   `email`, `size`, `tracks[]`, `nearAddress` (mainnet, payout), `projectUrl`,
   `demoUrl`, `idea` (20 to 2000 chars), `consent: true`. Never add `website`.

## Tracks chosen

| id | Track | Why Nearbee fits |
| --- | --- | --- |
| `agents` | Agent Economy | agents hired and paid by agents over A2A + x402; a playbook can hire other agents |
| `privacy` | Privacy & Private AI | private tier runs on NEAR AI Cloud TEE models with attestation evidence in the deliverable |
| `business` | Autonomous Business | an owner's playbooks earn on their own: intake, pricing, decline, delivery, payout without a human in the loop |

`open` is a fallback if the jury disagrees with the fit. `swarms` is plausible
once a playbook hires sub-agents on the market; list it only if that demo exists.

## Judging signals to design for

1. The README walkthrough works on a clean machine with Rust stable and `apb` installed.
2. NEAR's role is concrete in the demo: "paid in USDC on NEAR with x402",
   "ran in a TEE on NEAR AI Cloud, here is the attestation".
3. One complete moment in the video: a brief goes in, USDC moves, a deliverable
   with evidence comes out.
4. No marketing in the README.

## Credits and accounts needed

1. NEAR AI Cloud API key: https://cloud.near.ai (USD 10 of credits via
   https://nearbuilders.org/join).
2. A market builder account with a confirmed email at https://market.near.ai
   (publishing is gated on the confirmation); a GitHub + X link gives the
   "Social media verified" mark, which some board jobs require.
3. A NEAR mainnet account for the prize payout (named or implicit). Never created
   by an agent.
4. For the x402 demo caller: a NEAR mainnet account holding a few USDC
   (contract `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`)
   and an access key for it. The payer cannot be the agent's owner (the market
   refuses paying your own agent), so the demo needs a second market identity,
   which the caller's wallet becomes on its first call.

## Never without the owner's explicit confirmation

Send the submission; post on X; make the repository public or push to it; sign
transactions, spend tokens or deploy to mainnet; commit secrets.
