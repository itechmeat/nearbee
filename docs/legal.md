# Legal frame (the builder's reading, not legal advice)

Documents, all version 2026-10-15 unless noted: Terms of Service
(market.near.ai/terms), Builder Agreement (/builder-agreement), Standardized Job
Terms (/job-terms), Privacy Policy (/privacy). Builder FAQ last updated
2026-10-19. Quote the documents, not this summary, when it matters.

## Who contracts with whom

1. The **Job Agreement** is directly between the Agent Provider (the builder,
   that is the Nearbee owner) and the Buyer, on the Standardized Job Terms plus
   the listing as published at the moment of hire (timestamped and incorporated,
   Builder Agreement 3.2). The Operator (NEAR AI Agent Market) is not a party.
2. The Operator is "not a bank, deposit-taker, fiduciary, or escrow agent";
   held funds are in custodial wallets of the Wallet Provider, not insured (6.1,
   6.2.2). Users hold no keys (Terms 3.9).
3. Governing law Delaware, USA; disputes between Operator and Builder in Delaware
   courts (15.1, 15.2). Job disputes between builder and buyer go through the
   automated Resolver (6.9).

## What the builder is responsible for

1. **Everything behind the agent**, including "any third party, subprocessor, or
   subcontractor it engages to develop, host, or operate an Agent or to deliver a
   Job" (5.3, and indemnity 9(h)). Model providers, OCR services and connectors a
   playbook uses are the builder's subprocessors.
2. **Buyer data** (Standardized Job Terms 7.1 to 7.3): use it only to perform
   and support the job, no training or profiling (7.1); share with a
   subprocessor only as necessary and bound no less protectively (7.2); "on
   completion of the Job, the Agent Provider will delete or stop using the
   Buyer's inputs and other Buyer data", except where law or a vertical addendum
   requires retention (7.3). The terms do not prescribe keeping data through
   the acceptance and dispute windows; keeping it that long is Nearbee's own
   choice to be able to redo or defend the work, and must be stated in the
   listing. Connected Tools (7.6) are used only within the job.
3. **Listings must be accurate**: the listing is part of the contract, so a
   privacy claim in a description is a contractual promise.
4. Regulated verticals need the builder's own compliance (10).
5. Confidentiality of non-public market information (12).

## Money terms

1. Platform fee 5% of each completed job, deducted before payout (Schedule A A.1).
   Changes to the fee or payout terms need 7 days' notice and apply prospectively.
2. Operator liability toward the builder is capped (Builder Agreement 13.3,
   13.4). Builder liability toward a buyer for direct damages is capped at the
   job price (Job Terms 11.4), except the non-infringement representation and
   IP indemnity for the deliverable (8.3, 8.4, carved out in 11.5), fraud and
   wilful misconduct. Unless the listing assigns the deliverable, the builder
   keeps ownership and the buyer gets a perpetual non-exclusive licence on full
   payment (8.1).
3. The legal texts say the Held Funds are "not held in escrow" and the Operator
   is no escrow agent (Job Terms 3.2); the API's word "escrow" is a label. A
   listing may state its own refund policy (5.1).
4. Dispute Deposit 5% of the job budget (min 0.01); forfeited to the Operator only
   when the ruling is entirely against the depositor (6.9).
5. The Operator may refund a buyer and recoup from the builder (6.8).

## Consequences for Nearbee's design

1. The adapter must be able to **state its subprocessors per listing**: which
   model providers, which connectors, where they run. `docs/privacy.md` derives
   that statement from the playbook's profiles and connectors and puts it in the
   listing and in every deliverable.
2. The adapter must **delete job data on schedule**: brief, attachments, run
   workspace, deliverable copies, after `autoAcceptAt` plus the dispute window,
   and record the deletion in the hire journal.
3. **No training, no reuse**: apb's corrections ledger, memory and judge examples
   must not ingest buyer material from market runs. The runner marks market runs
   so apb's self-improvement features skip them (an apb change to verify; see
   `docs/apb.md`).
4. Descriptions are generated from what the adapter enforces, never typed
   freely, so the listing cannot promise more than the tier guarantees.
