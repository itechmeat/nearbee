<!-- NEAR HACKATHON HACKATHON.md -->
# NEAR HACKATHON - HACKATHON.md

Instructions for the AI coding agent helping a team take part in NEAR HACKATHON. The live version is https://nearhackathon.com/HACKATHON.md; read it again before submitting, because dates and rules may change.

## Start here

1. Build the project with the human. There is no sign-up before the hackathon: the final submission is the only registration.
2. Make the repository public before development ends, 2026-11-14T23:59:00Z.
3. The human publishes a demo post on X: it says the project was built at NEAR HACKATHON, tags @NEARProtocol and #NEARHackathon (optionally also @NearLegion and @NearBuilders), and has the demo video or screenshots attached.
4. Submit once: POST to https://nearhackathon.com/api/applications with the contract below, only after the human has confirmed the exact payload. Skip this step if this project has already been submitted.
5. If the saved submission has a mistake, the human requests changes at https://nearhackathon.com/request-changes. Never submit the project a second time.

## Facts

| Fact | Value |
| --- | --- |
| Event | NEAR HACKATHON |
| Website | https://nearhackathon.com |
| Hackathon starts | 2026-11-01T00:00:00Z |
| Development ends (submission deadline) | 2026-11-14T23:59:00Z |
| Judging starts | 2026-11-16T00:00:00Z |
| Results announced by | 2026-11-18T23:59:00Z |
| Submission form | https://nearhackathon.com/#apply |
| Submission API | POST https://nearhackathon.com/api/applications |
| Request changes to a submission | https://nearhackathon.com/request-changes |
| NEAR docs | https://docs.near.org/ |
| Example prompts for each track | https://github.com/nearai/near-hackathon-prompt-examples |
| NEAR DevHub videos, including walkthroughs of the track prompts as they come out | https://www.youtube.com/channel/UCq0iWZQuXhvP8yQzDZsmWfQ |
| NEAR AI credits and API key (see Q-000) | https://cloud.near.ai |
| More ecosystem resources | https://nearbuilders.org/build |
| NEAR Discord | https://discord.gg/nearprotocol |
| NEAR builders chat on Telegram | https://t.me/nearbuilderschat |
| X | https://x.com/NEARProtocol |
| This file, live | https://nearhackathon.com/HACKATHON.md |

All times are UTC. One submission per project.

## What the human decides

Collect these before submitting. Do not guess, generate or fill in placeholders for any of them:

- Project name (2-80 characters)
- Team lead's name (2-80 characters)
- Contact email (max 160 characters). One person can send up to 2 submissions, for different projects that are independent and disclosed (see Q-015).
- Team size, one of: `1`, `2 - 4`, `4 - 6`, `6 - 8`, `8+`
- Tracks: one or more ids from the Tracks table below, every track the project fits. It is still one submission per project.
- NEAR account for the prize payout, mainnet: a named account such as `team.near` or a 64-character implicit account. The team creates it in any NEAR wallet; see https://docs.near.org/. Never create or invent one on their behalf.
- Link to the public repository judges will review. Each repository can be submitted only once.
- Link to the public demo post. The human writes and publishes it themselves.
- What they built (20-2000 characters): the problem, the product, and how NEAR fits in.
- Consent to the contact notice, read to them verbatim: "I agree that the organizers may use these details to review our submission and contact us about this hackathon."

## Never without the human's explicit confirmation

- Send the submission. Print the full JSON payload first and wait for a yes. A second submission of the same repository is rejected, so there is one attempt per project. Mistakes are fixed by requesting changes, not by submitting again.
- Post anything on X or another network on their behalf.
- Make the repository public, push, or force-push.
- Sign transactions, spend tokens or deploy to mainnet.
- Commit secrets. Keys belong in environment variables, never in the repository.

## Rules

**Q-000. NEAR AI credits?** Buy with crypto or card in the NEAR AI Cloud dashboard (https://cloud.near.ai), or claim $10 by joining NEAR Builders (https://nearbuilders.org/join). Generate the API key in the dashboard.

**Q-001. Code before 2026-11-01?** Yes, from the announcement on 2026-10-19 (https://x.com/nearprotocol/status/2112257967818997891). The window runs from then to 2026-11-14. Earlier commits are baseline.

**Q-002. Existing product on another chain?** Allowed. Only the NEAR integration built in the window is judged. Disclose in README.

**Q-003. New repo required?** No. Dedicated branch, or tags pre-hackathon and hackathon-submission, plus a README section linking the diff.

**Q-004. Old project or new feature on it?** Old project as-is: no. New feature in the window: yes, if separated per Q-003. No minimum code size.

**Q-005. Libraries, templates, boilerplate?** All allowed, including own pre-hackathon libs and AI code. List in README; own old libs count as baseline.

**Q-006. 1Click JWT?** Not required. Without it, 0.2% fee. Register at https://partners.near-intents.org.

**Q-007. Testnet for Intents?** None exists or is planned. Mainnet, separate dev accounts, tiny amounts, dry: true quotes.

**Q-008. Test funds?** Testnet NEAR is free (near-cli faucet, near-faucet.io). Mainnet costs not reimbursed by default. (see Q-007)

**Q-009. RPC and hosting?** Public RPC free: https://docs.near.org/api/rpc/providers

**Q-010. Testnet-only projects eligible?** Yes, all tracks. Mainnet only where the integration is mainnet-only.

**Q-011. Mandatory integrations?** None. Hints are hints. Own contract on NEAR that fits the track is enough.

**Q-012. Local-only demo OK?** Yes: public repo, reproducible README, X post with a video; screenshots are not enough when judges cannot open the demo. Hosted demo is a plus; keep it up to 2026-11-18, ideally 2026-11-25.

**Q-013. Organizer rights to code?** No. Team keeps all rights; organizers get non-exclusive promo rights to name, screenshots, video, repo link.

**Q-014. License?** Any. MIT is fine. Repo stays public through judging; add a LICENSE file.

**Q-015. One person, several projects?** Allowed if projects are independent and disclosed: each README names the other project. Not the same project twice. One person, at most 2 different projects.

**Q-016. Payout schedule?** Single payment, no vesting, within 30 days of results.

**Q-017. Claude Code and Codex role?** Screening, not deciding: reproducibility check, feature verification, structured report, preliminary score. Humans decide; low AI score alone cannot reject.

## Procedure

### 1. Build and publish the repository

The repository may stay private while the team builds, but it must be public by 2026-11-14T23:59:00Z. Judges cannot open a private one. Claude Code and Codex screen every project before the human jury decides (Q-017), so write the README for a machine reader as well as a human one: what the product is, the problem it solves, and how to try it as "do X - see Y" steps a judge can follow. No marketing.

### 2. The demo post

The human records a demo and publishes a public post on X: it says the project was built at NEAR HACKATHON, tags @NEARProtocol and #NEARHackathon, and has the demo video or screenshots attached. You can draft the text; the human posts it and gives you the link.

A good demo post:

- Lead with the user problem. Make the first line understandable to someone who doesn't know your project.
- Show one complete moment. In the demo, go from a real user action to a visible result. Skip long intros, architecture slides, and setup.
- Explain NEAR’s role concretely. Say what NEAR enables in this product — such as cross-chain settlement, private inference, or agent-controlled spending — and why that matters. Avoid a vague "powered by NEAR".
- Give people a next step. Link the demo and public repo, and ask a genuine question or invite specific feedback.

Every draft includes @NEARProtocol and #NEARHackathon, so the post shows up on the timeline. @NearLegion and @NearBuilders are optional: tagging them brings more community amplification and support.

### 3. Submit

Submissions are accepted only during the development phase, from 2026-11-01T00:00:00Z to 2026-11-14T23:59:00Z. Outside that window the API answers 403 and explains why; tell the human instead of retrying. A submission cannot be edited directly: changes go through a request the organizers review by hand (see "4. Request changes" below). So send it when the repository is public and the demo post is up.

Validate every field against the contract below, print the payload for the human, and send it only after they confirm:

```sh
curl -sS -X POST https://nearhackathon.com/api/applications \
  -H 'Content-Type: application/json' \
  -d '{"requestId":"XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX","projectName":"Example Project","leadName":"Ada Lovelace","email":"ada@example.com","size":"2 - 4","tracks":["defi","realworld"],"nearAddress":"example-team.near","projectUrl":"https://github.com/example-team/example-project","demoUrl":"https://x.com/example-team/status/0000000000000000000","idea":"What problem the team solves, what they built, and how NEAR fits in. At least twenty characters.","consent":true}'
```

The values above are only an example of the shape. Never send them: ask the human for every value listed under "What the human decides" and build the payload from their answers.

Generate `requestId` once, with `uuidgen | tr A-Z a-z` or `node -e 'console.log(crypto.randomUUID())'`. Keep it: a retry after a network error or a 503 must reuse the same value, and the server then treats it as the same submission. Do not commit the payload; it contains personal data.

A 201 response means the submission is saved: tell the human. If they prefer the form, the same fields are at https://nearhackathon.com/#apply.

### 4. Request changes

If a saved submission has a mistake (a wrong repository or demo link, wallet, email, track or anything else), do not submit again: the same repository is rejected with 409, and a different link would count as another project. The human requests changes at https://nearhackathon.com/request-changes instead. There they enter the email, NEAR account and repository link from the submission exactly as submitted; if the mistake is in one of these, they enter what they submitted as best they remember. Then they tick every field to change, give its new value, and leave a Discord or Telegram handle. One request can change several fields.

New values follow the same rules as the submission (see Submission API below), so check them with the human before they send. The organizers review every request by hand and may message the human to confirm, especially before changing the payout account or the repository. Until a change is applied, the submission stays as it was. Help the human gather the values; the human sends the request from the page.

## Submission API

`POST https://nearhackathon.com/api/applications` with `Content-Type: application/json`. Body up to 12000 bytes. Send exactly these fields:

| Field | Type | Rule |
| --- | --- | --- |
| requestId | string | UUID, generated once per submission and reused on retries |
| projectName | string | 2-80 characters |
| leadName | string | 2-80 characters |
| email | string | valid email, max 160 characters, at most 2 submissions per email |
| size | string | one of `1`, `2 - 4`, `4 - 6`, `6 - 8`, `8+`, exactly as written |
| tracks | array of strings | one or more of `defi`, `thesis`, `privacy`, `business`, `swarms`, `security`, `agents`, `realworld`, `games`, `nfts`, `memecoins`, `social`, `open`, each listed once |
| nearAddress | string | 2-64 characters, lowercase, matching `^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$` |
| projectUrl | string | http(s) URL of the public repository, max 300 characters, unique: each repository is submitted once |
| demoUrl | string | http(s) URL of the public demo post, max 300 characters |
| idea | string | 20-2000 characters |
| consent | boolean | `true`, only after the human agreed to the contact notice |

Do not add other fields. In particular `website` is a spam trap: any non-empty value rejects the submission.

| Status | Meaning | What to do |
| --- | --- | --- |
| 201 | Submission saved | Done. Tell the human. |
| 400 | A field failed validation | Fix the fields listed in `fields` of the response and resend with the same requestId. |
| 403 | Submissions are not open, or the request looked cross-site | Tell the human. Do not retry. |
| 409 | Repository already submitted, or this email already has 2 submissions | The project was already submitted, or the human reached the limit. Ask the human; do not change the link or the email to get around it. If the earlier submission has a mistake, the human requests changes at https://nearhackathon.com/request-changes. |
| 413 | Body too large | Shorten the description. |
| 503 | Storage temporarily unavailable | Wait a minute and resend with the same requestId. |

## Tracks

Put the `id` of every track the project fits in the `tracks` array of the submission. It is still one submission per project.

The Hints column is what the organizers hope to see in each track: NEAR features and themes worth building around. They are hints and wishes, not requirements, and so are the protocols named in the Build column (Q-011). Lean on the ones that fit the idea and name them in the README.

The Prompt column links an example prompt for the track, with links to relevant resources, from https://github.com/nearai/near-hackathon-prompt-examples. It is a starting point, not a requirement.

| id | Track | Build | Hints: what to lean on | Prompt |
| --- | --- | --- | --- | --- |
| `defi` | Unstoppable DeFi | Build exchanges, perps, lending, vaults and trading agents that settle across chains through NEAR Intents. Deep liquidity, no custodian, nobody to switch it off. | TRADING, NEAR INTENTS, CHAIN SIGNATURES | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/01-defi.md |
| `thesis` | Investment Thesis | Build a long-term investment thesis for NEAR and back it with data: onchain analytics, live dashboards and models anyone can re-run. Judged on the evidence, not the conviction. | ONCHAIN DATA, DASHBOARDS, RESEARCH | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/02-thesis.md |
| `privacy` | Privacy & Private AI | Explore confidential computation, private finance and AI that protects sensitive data. Build intelligence without exposure. | PRIVATE AI, TEE, NEAR AI CLOUD | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/03-privacy.md |
| `business` | Autonomous Business | Build AI-native businesses with treasury, sales, procurement, cross-chain payments via NEAR Intents and revenue sharing. Humans only set the constraints. | AI AGENTS, NEAR INTENTS, CHAIN SIGNATURES | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/04-business.md |
| `swarms` | AI Swarms | Build coordination markets, bidding, collective purchasing, intent-based settlement and emergent economies between agents. | MULTI-AGENT SYSTEMS, NEAR INTENTS, SUB-ACCOUNTS | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/05-swarms.md |
| `security` | AI Security & Defense | Build spending limits, policy engines, sandboxed wallets, reputation and scam detection. Give AI money without giving it the keys to everything. | AGENT SECURITY, FUNCTION CALL KEYS, TEE ATTESTATION | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/06-security.md |
| `agents` | Agent Economy | Build agents that earn, spend, negotiate, hire other agents and pay for APIs, compute or data — with crypto directly or through NEAR Intents. | AGENT COMMERCE, NEAR INTENTS, CHAIN SIGNATURES | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/07-agents.md |
| `realworld` | Real-World Money | Build payroll, invoicing, remittances, merchant payments and stablecoin rails for real businesses. Something a normal company could use tomorrow. | STABLECOINS, NEAR INTENTS, META TRANSACTIONS | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/08-realworld.md |
| `games` | Games With Real Economies | Build NPCs with wallets, player-owned markets and economies that evolve without a dev patch. Judged on the game loop, not the token — bonus if players never notice the chain. | AGENT NPCS, SOCIAL LOGIN, NEAR INTENTS, NEP-245 | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/09-games.md |
| `nfts` | NFTs That Do Something | Build NFTs as accounts — assets that own money, earn royalties, hold inventory, carry credentials, memberships or access rights. Ownership with behavior. | DYNAMIC NFTS, NEP-171, SUB-ACCOUNTS | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/10-nfts.md |
| `memecoins` | Memecoins With Mechanics | Build memecoins where the meme does something — supply that reacts to the community, treasuries with rules, onchain rituals and challenges. Judged on the mechanic, not the chart. | MEMECOINS, TOKEN MECHANICS, NEP-141 | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/11-memecoins.md |
| `social` | Social Money | Build shared budgets, programmable rewards, collective ownership and reputation-based access on top of Sputnik DAO and Trezu. Don't rebuild the multisig — build what communities do with it. | SPUTNIK DAO, TREZU, NAMED ACCOUNTS | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/12-social.md |
| `open` | Open Innovation | Have an idea that refuses to fit a category? Explore new ways for money, intelligence and ownership to work together. | EXPERIMENTAL, CHAIN ABSTRACTION, BUILD YOUR OWN | https://github.com/nearai/near-hackathon-prompt-examples/blob/main/tracks/13-open.md |

## Prizes

Prize pool $250,000. One grand prize overall. 1st, 2nd and 3rd place in each of the 13 build tracks. Amounts are fixed in USD and paid in NEAR to the account given in the submission (see Q-016).

| Prize | Amount | Count | Total |
| --- | --- | --- | --- |
| Grand prize | $55,000 | 1 | $55,000 |
| 1st place | $10,000 | 13 | $130,000 |
| 2nd place | $3,000 | 13 | $39,000 |
| 3rd place | $2,000 | 13 | $26,000 |

## Ecosystem

Partner tools and their documentation. Read the docs before integrating anything; verify endpoints, contract addresses and SDK versions there instead of assuming them. More ecosystem resources, updated as they become available: https://nearbuilders.org/build.

| Partner | What it is | Docs | Website | GitHub |
| --- | --- | --- | --- | --- |
| NEAR Intents | Cross-chain execution | https://docs.near-intents.org/ | https://intents.near.org/ | https://github.com/near/intents |
| Aurora | Ethereum on NEAR | https://doc.aurora.dev/ | https://aurora.dev/ | https://github.com/aurora-is-near |
| HOT | Chain abstraction | https://docs.hotdao.ai/ | https://hot-labs.org/ | https://github.com/hot-dao |
| Rhea | DeFi on NEAR | https://guide.rhea.finance/ | https://rhea.finance/ | https://github.com/rhea-finance |
| meme.cooking | Memecoin launchpad | https://docs.meme.cooking/ | https://meme.cooking/ |  |
| HEX | Cross-chain swaps | https://hot-labs.gitbook.io/hex/concepts-and-systems/interactive-blocks | https://hex.exchange/ |  |
| NEAR.AI | Private AI | https://docs.near.ai/ | https://near.ai/ | https://github.com/nearai/cloud-api |
| SVRN | Sovereign technology | https://svrn.net/research/near-protocol | https://svrn.net/ |  |
| Trezu | Multisig treasury for Web3 teams | https://github.com/NEAR-DevHub/trezu-docs | https://trezu.org/ | https://github.com/NEAR-DevHub/trezu |
| SputnikDAO | DAO infrastructure | https://github.com/near-daos/sputnik-dao-contract#setup | https://x.com/SputnikDAO | https://github.com/near-daos/sputnik-dao-contract |
| OutLayer | Secure transactions for AI agents | https://app.outlayer.ai/docs/getting-started | https://outlayer.ai/ | https://github.com/out-layer/outlayer |
| GoMining | Bitcoin mining superapp | https://docs.gomining.com/ | https://gomining.com/ |  |
| Agent Market | Marketplace for AI agents | https://docs.market.near.ai/ | https://market.near.ai/ |  |
| NEAR Builders | Builder community | https://nearbuilders.org/build | https://nearbuilders.org/ | https://github.com/nearbuilders |
| Gulf Labs | Invests in blockchain infrastructure |  | https://gulf-labs.com/ |  |
| BRRRAIN | Memecoin with AI rewards | https://brrrain.fun/docs/ | https://brrrain.fun/ |  |
| NEAR Business | Confidential multisig treasury | https://docs.business.near.com/ | https://business.near.com/ |  |

### Partner notes

- Rhea (rhea.finance): NEAR's DEX and lending protocol. Looking for something to build? Consider CLMM and AMM pools, the cross-chain swap API or vaults on top of its lending. SDKs and API docs: https://github.com/rhea-finance/rhea-sdk-docs. CLI: https://guide.rhea.finance/developers-1/cli-trading
- Agent Market (market.near.ai): a marketplace where AI agents get hired and paid by the job, by people and by other agents. List the agent you build so it can earn: https://docs.market.near.ai/list-your-agent. Hire agents from Claude or other MCP clients: https://docs.market.near.ai/guides/claude-desktop
- NEAR Business (business.near.com): a confidential multisig built on NEAR Intents. It keeps treasury balances and transfers hidden from the public, manages assets on 35+ chains from one dashboard and sets approval policies that fit your treasury. Sign up: https://business.near.com/create?ref=og. Demo: https://app.trupeer.ai/view/OEoiJVjUh/near-business-create-and-manage-a-treasury

## Community

Where the team finds collaborators, asks questions and follows updates.

| Channel | Link | What it is for |
| --- | --- | --- |
| NEAR Discord | https://discord.gg/nearprotocol | Questions about building on NEAR, finding teammates |
| Telegram | https://t.me/nearbuilderschat | Questions about building on NEAR, finding teammates |
| X | https://x.com/NEARProtocol | Hackathon announcements and updates |

