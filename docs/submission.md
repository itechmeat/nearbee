# Submission payload (draft)

The hackathon submission is sent once, only after the owner has read this
exact payload and said yes (`docs/hackathon.md`, O-9). Fields marked
`<owner>` are the owner's to fill; nothing here is sent by an agent.

| Field | Value | Source |
| --- | --- | --- |
| `requestId` | generated once by the owner's send step (UUID) and kept with the sent payload | `docs/hackathon.md` |
| `projectName` | `Nearbee` | this repository |
| `leadName` | `<owner>` | owner |
| `email` | `<owner>` | owner |
| `size` | `<owner>` | owner |
| `tracks` | `["agents", "privacy", "business"]` | `docs/hackathon.md`, Tracks chosen |
| `nearAddress` | `<owner>` (mainnet payout account, O-4) | owner |
| `projectUrl` | `https://github.com/itechmeat/nearbee` | this repository |
| `demoUrl` | `<owner>` (the X post with the video, O-8) | owner |
| `idea` | the text below | this file |
| `consent` | `true` | `docs/hackathon.md` |

No `website` field.

## idea

Nearbee turns an Agentic Playbooks (apb) playbook into an agent for hire on
the NEAR AI Agent Market. One command lists a playbook with its price,
delivery window and model profile; `nearbee serve` checks each brief
against the listing before it accepts the hire, runs it as an apb run with
a cost ceiling derived from the price, and submits the result as Markdown
and files. Buyers pay in USDC on NEAR through the market's held payment,
including agent-to-agent over A2A with x402. Each listing has a privacy
tier the adapter enforces before it publishes: attested listings call only
NEAR AI Cloud TEE models from Nearbee's own commands, and the deliverable
carries the model's response signature with a Verification Center receipt.
Runs execute in a container with no access to the owner's files and an outbound host allowlist, and the listing's data statement says so. Every hire is journaled and survives a restart; every market run is an apb
external-data run, so apb does not learn from it. Two example agents show it: a
repository audit with checked file and line citations, and a private
document digest. `nearbee replay` walks the whole hire machine on a clean
checkout without any account.
