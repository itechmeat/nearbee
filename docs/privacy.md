# Privacy tiers

A buyer's question is "where does my data go, and how do I know". The legal
frame (`docs/legal.md`) makes the owner responsible for every subprocessor and
makes the listing a contract. Nearbee answers the question with tiers it
validates, statements it generates, and evidence it attaches. The claims below
are deliberately no stronger than what the adapter can show.

## Tiers

| Tier | Who can read the buyer's data | Model steps | Other steps | Evidence in the deliverable |
| --- | --- | --- | --- | --- |
| **plain** | the owner's machine, the owner's model providers (any) | any profile | any connector | the data statement |
| **attested** | the owner's machine, NEAR AI Cloud Confidential TEE models | profiles whose every executor and fallback runs on `cloud-api.near.ai` Confidential TEE models (`owned_by: nearai`); decision providers likewise | no connectors unless on the listing's allowlist (empty by default) | statement + **run-level attestation disclosure**: the attestation reports of the models used, fetched at run start and end, the model ids apb journaled per node, a Verification Center link. This shows *which* models and that those endpoints attest as TEEs; it does **not** bind this job's responses to a TEE |
| **attested, signed** (approved for `doc-digest-private`, D-008) | as attested | one bounded model step done by a trusted `script` node that calls the Cloud API itself, keeps the exact request and response bytes and fetches the `provider_tee` response signature | as attested | statement + the response signature and the bytes' hashes: per-response proof that *this* inference ran in a model TEE |
| **blind** | NEAR AI Cloud TEE models only; the owner's machine relays ciphertext | one E2EE model step | no connectors; scripts touch only ciphertext | out of scope for the hackathon: a separate design (`docs/open-questions.md`) |

### What the validator checks at publish time

1. `attested`: every profile referenced by the materialised playbook's nodes
   (executor, fallbacks, tiers, routed sub-playbooks) resolves to NEAR AI Cloud
   Confidential TEE models; the decision provider configured for the apb project
   is on the same list or disabled; the connector set reachable by the playbook
   is within the allowlist. Incognito models (Claude, GPT, Gemini via the
   gateway) and `attested 3p` models are refused for this tier.
2. The playbook has no `human_review` node and no interactive node for listings
   open to A2A callers.
3. The published data statement equals the one generated from the current
   playbook, profiles and connectors; `serve` refuses hires otherwise.

### What the validator cannot enforce, said plainly

apb's `environment` is not a sandbox: an agent CLI or a script can call any
HTTP endpoint with any local credential. Validation covers declared bindings;
it does not prove the absence of other egress. The data statement says so:
"model steps are configured to run on NEAR AI Cloud TEE models; the run happens
on the owner's machine, which can read the data". An enforced boundary needs a
dedicated execution environment with egress control; that is post-hackathon
work and is listed as such.

## The data statement

Generated, never typed. For a listing it lists, from apb's data:

1. Executors: agent CLI, provider, model ids, where the model runs (owner's
   subscription at provider X; NEAR AI Cloud Confidential TEE; 3P TEE;
   incognito), with the catalog tier names, and the decision provider.
2. Connectors the playbook can call and what they receive.
3. Where the run happens: "the owner's self-hosted machine".
4. What the platform holds: the brief, thread messages and uploaded files stay
   with the market under its own policy; a file once used by a job cannot be
   deleted there, and its link works for anyone holding it.
5. Local retention: "deleted N days after the market reports acceptance;
   deletion is journaled", with the inventory below (what Nearbee deletes and
   what it cannot).

The statement goes into the listing's `bio_markdown` under "Data handling" and
into every deliverable's footer.

## Evidence

- Attested tier (v1): at run start and end, `GET /v1/attestation/report?model=`
  for each model in the tier set; the model ids used per node from apb's
  journal; a Verification Center link per model. Labelled in the deliverable as
  "endpoint attestation disclosure".
- Attested, signed (D-008, the digest step of `doc-digest-private`): the
  script node records request bytes, response bytes,
  the response id and the `provider_tee` signature, and writes them into the
  manifest's evidence entries; the deliverable carries hashes and the
  signature, the full bytes stay with the owner for the retention window.
- Journal hashes: `.nearbee/hires/<id>/journal.jsonl` holds hashes of every
  input, output and deletion with timestamps. Anchoring on NEAR is a stretch.

## Deletion inventory (as built in S3)

`nearbee hires` lists every journaled hire (read only, no lock): hire id,
agent handle, state, latest run, spent cost, deadline and an attention code;
never the brief text, a token or a capability link (`--json` for the same as
JSON).

`nearbee cleanup` takes the serve lock (`<state_dir>/serve.lock`, exit 4 while
a serve or another cleanup holds it) and deletes, for each eligible hire, only
the paths Nearbee owns:

1. the hire's worktree under `<workspace>/work/` (the buyer's `inputs/`, the
   run's `out/`, anything else the run wrote there): every worktree the hire's
   run intents journaled (an S2 hire's per-execution path included) and the
   per-hire default;
2. the hire's snapshot directories `<state_dir>/hires/<hire>/gen-*` (frozen
   submissions, run request bodies, the previous generations' `out/`).

A journaled worktree outside `<workspace>/work/` after canonicalisation is
never touched and is reported as refused. The hire journal stays (hashes,
timestamps, states) and gets a `deleted` record listing every path removed,
already absent, failed or refused, and what is not covered. A path counts
as removed or absent only once the directory that held it is synced, and
snapshot directories that cannot be listed are a failure, never an empty
list. A hire with a failed path stays eligible and the next cleanup retries
it.

Eligibility (`retention.after_acceptance_days`, default 7):

1. closed `accepted`: that many days after `finalizedAt` (else after the
   acceptance was first observed); acceptance is never inferred from
   `autoAcceptAt`;
2. closed `cancelled` with no delivery, a decline at intake included: at once;
3. closed `cancelled` after a delivery, or `dispute_resolved`: that many days
   after the closing was first observed;
4. never: a frozen (disputed) hire, a hire with an open intent, a hire still
   open or delivered and not yet closed.

`nearbee cleanup --dry-run` prints the same plan, takes no lock and changes
nothing.

Not covered, listed by name in every `deleted` record and in the dry run:

1. apb's run directories of the hire's run ids (apb `1.0.0-beta.6` has no run
   deletion, `docs/apb.md`);
2. apb's `.apb/decisions.jsonl` entries of the scope check (digests and
   metadata of the question, never deleted by Nearbee);
3. caches the executors wrote (agent CLI memory and session files).

Market playbooks run on Nearbee's own apb instance (separate config and data
directory, `docs/apb.md`), so nothing they produce reaches the operator's
everyday apb. apb itself learns from every run today with no switch to turn it
off (S1 finding); apb's `near-ai-cloud-models` change adds an `external_data`
run flag that excludes a run from every learning and reuse path (corrections,
known problems, stats and estimates, judge examples, scenario capture, run
memory) and keeps it visible on the run (D-023). Nearbee sets that flag on
every market run once apb ships it and checks it on the run detail; until then
only the owner's own test hires run. The flag does not cover an agent CLI's
own memory and session files: those are the executor caches above.

## What is honestly not guaranteed

1. In `plain` and `attested`, the owner's machine sees the data.
2. `attested` shows which model endpoints attest as TEEs and which models ran;
   it does not prove per response unless the signed path is used.
3. Platform-held copies (brief, thread, files) are outside the owner's control.
4. Evidence depends on what the executor exposes; the signed path exists
   because general CLIs hide the bytes.
