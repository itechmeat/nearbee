# Proposal

## Why

Nearbee serves hires, but its only listing is a placeholder that runs on
the owner's subscription under a "fast" name, and nothing checks that a
listing's privacy promise matches what its playbook can reach. Stage S4
(`docs/stages.md`, plan items 13 to 15) ships the first real example agent
and makes the privacy tier a checked property of the listing, so the demo
and the judges see a promise the adapter enforces and evidences.

## What Changes

1. **Variants**: a listing file names its `tier` (`plain` or `attested`,
   required, **BREAKING** for listing files) and optionally a `profile`;
   one source playbook materialises into one apb playbook per listing with
   that profile as its default.
2. **Profiles**: a third template `nearbee-own-premium` (Claude Opus on the
   owner's subscription); `nearbee-near` runs DeepSeek V4.1 Flash first and
   Qwen3.8-27B as fallback.
3. **Inspection seam**: the `Runner` reports the execution facts of an
   installed playbook (nodes, every profile it can use with executor,
   fallbacks and tiers, the model route, decisions and compaction settings),
   with explicit unknowns.
4. **Tier validator**: `nearbee list` refuses a listing whose facts break
   its tier, before and after installing; `serve` re-checks at start and
   before every apb start and stops taking new work for a drifted listing.
5. **Data statement**: generated from the same facts, printed for the
   web-app bio, recorded with a fingerprint, snapshotted per generation and
   appended to every deliverable.
6. **Run evidence** (attested tier): endpoint attestation reports for every
   usable model, fetched before the apb start and after the run, kept
   privately, disclosed in the deliverable with the models apb requested
   per node and a Verification Center link; fail closed.
7. **Agent A `repo-audit`**: `playbooks/repo-audit/` with listings
   `repo-audit-fast` (attested, `nearbee-near`) and `repo-audit-premium`
   (plain, `nearbee-own-premium`); a GitHub snapshot fetch and a findings
   check and render step as Nearbee commands called by trusted script
   nodes; the findings JSON embedded in the Markdown deliverable. The S2
   placeholder `playbooks/first-hire` is removed.
8. **Docs**: the signed step of D-008 is specified as a contract for S5;
   `docs/privacy.md`, `docs/showcase-agents.md`, `docs/stages.md`,
   `docs/owner-checklist.md` (O-11 reworded: no apb profile setting jails
   files) and `docs/apb.md` updated.

## Capabilities

### New Capabilities
- `privacy-tiers`: the tiers, the facts a tier is checked against, and when the checks run.
- `data-statement`: the generated statement, its fingerprint, and where it appears.
- `run-evidence`: endpoint attestation evidence for attested hires and its failure rules.
- `repo-audit`: the example agent's repository snapshot, findings contract and deliverable.

### Modified Capabilities
- `listing-publish`: listing files gain `tier` and `profile`; materialisation applies the profile; publishing validates the tier and prints the statement.
- `executor-profiles`: three stable profile names; the near profile's model order.
- `hire-serving`: the deliverable carries the statement and evidence; drifted listings take no new work.
- `apb-runner`: run observation keeps the models apb requested per node attempt.

## Impact

1. Crates: `nearbee-core` (new modules for tiers, statement, evidence; the
   `Runner` and a new `Attestor` seam; serve), `nearbee-apb` (inspection,
   materialisation, `attempt_models`), a new crate `nearbee-cloud` (the
   NEAR AI Cloud attestation client behind `Attestor`), `nearbee` (listing
   fields, publish, `audit fetch` and `audit finish` commands). The seam
   rule holds: the core sees only `Runner`, `Market` and `Attestor`.
2. New dependencies: `flate2` and `tar` for the GitHub tarball.
3. The recorded listing gains tier, statement hash and fingerprint; an older
   record takes no new hires until it is listed again.
4. Out of scope: the signed step's implementation (S5, contract only here);
   market file uploads; active decision-provider chains and connector
   allowlists for attested listings (refused, not analysed); per-hire
   parallel runs (D-025); the blind tier and the native apb NEAR executor
   (D-011, not released); market writes (D-024); third-party hires before
   apb ships `external_data` (D-023).
