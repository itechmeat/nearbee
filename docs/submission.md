# Submission payload (draft)

The hackathon submission is sent once, only after the owner has read this
exact payload and said yes (`docs/hackathon.md`, O-9). Fields marked
`<owner>` are filled by the owner at send time and are never committed
(personal data, and the X post that is not published yet); nothing here
is sent by an agent.

| Field | Value | Source |
| --- | --- | --- |
| `requestId` | generated once by the owner's send step (UUID) and kept with the sent payload | `docs/hackathon.md` |
| `projectName` | `Nearbee` | this repository |
| `leadName` | `Serg Eroshe` | owner |
| `email` | `<owner>` | owner |
| `size` | `1` | owner |
| `tracks` | `["agents", "privacy", "business"]` | `docs/hackathon.md`, Tracks chosen |
| `nearAddress` | `techmeat.near` (mainnet payout account, O-4) | owner |
| `projectUrl` | `https://github.com/itechmeat/nearbee` | this repository |
| `demoUrl` | `<owner>` (the X post with the video, O-8) | owner |
| `idea` | the text below | this file |
| `consent` | `true` | `docs/hackathon.md` |

No `website` field.

## idea

1380 characters (limit 20 to 2000), counted on the text below as one
paragraph: the line breaks here are for reading and become spaces.

Nearbee turns an Agentic Playbooks (apb) playbook into an agent for hire
on the NEAR AI Agent Market. `nearbee list` publishes a playbook as a
listing with its price, delivery window, the models its steps call and a
data statement generated from the playbook. `nearbee serve` checks each
brief against the listing before it starts, runs the hire as an apb run
with a cost ceiling derived from the price, and submits the result as
Markdown plus output files. It can also bid on matching jobs on the
market's job board and serve an award as a hire. Buyers pay in USDC on
NEAR through the market's held payment, including agent-to-agent over
x402. Runs execute in a container with no access to the owner's files and
an outbound host allowlist. Each listing has a privacy tier the adapter
enforces before publishing: attested listings call only NEAR AI Cloud TEE
models from Nearbee's own commands, and the deliverable carries the
model's response signature with a Verification Center link. Every hire is
journaled and survives a restart. Recorded paid hires over x402: a
repository audit (0.50 USDC, attested, signature checked), a bug fix with
a failing test (2.00 USDC, accepted) and a dependency upgrade that took
npm audit from 29 vulnerable packages to 5 low (1.00 USDC, accepted).
`cargo run -p nearbee -- replay` walks the hire machine on a clean
checkout without any account.
