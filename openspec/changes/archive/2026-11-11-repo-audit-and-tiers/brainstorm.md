# Brainstorm

## Problem

S4 must turn Nearbee from "runs any playbook" into "runs a real example
agent with an honest privacy promise": agent A (`repo-audit`) in two
variants, a tier validator that refuses a listing whose bindings break its
tier, a generated data statement, and run-level evidence for the attested
tier. The implementer is a GLM-class coding agent that cannot build in this
repository, so every behaviour has to be specified tightly enough to write
without guessing, and the scope has to fit about one day.

## Related work

1. `docs/stages.md` S4; plan items 13 to 15.
2. Decisions D-007 (tiers, run-level disclosure), D-008 (signed step),
   D-011 (out of v1), D-013 (materialised variants), D-014 (manifest),
   D-018 (owner prices), D-023 (owner test hires only until apb ships
   `external_data`), D-024 (no market writes), D-025 (one run slot).
3. Archived changes: S3 `durable-hire-machine` left two items for S4
   (parallel runs, output file uploads); S2 `market-and-first-hire` shipped
   the placeholder `repo-audit-fast` on the owner's subscription.
4. apb is at `v1.0.0-beta.6`: its native NEAR AI Cloud executor and the
   `external_data` run flag are merged upstream but not released.
5. No open pull requests.

## Doubts

1. Where the validator gets profile, connector and decision-provider facts
   in apb beta.6, and what to do when apb cannot tell.
2. Whether a statement equality check at serve time is worth it.
3. What happens to an attested hire when an attestation fetch fails.
4. Whether the market file trio is safe enough to add now.
5. Exact bytes and the signature endpoint of the signed step.
6. Fetching a buyer-named repository safely (SSRF, size, archive tricks).
7. Prompt injection from repository content into the audit agent and its
   findings.
8. How much a GLM implementer can carry in one pass.

## Approaches considered

1. Variants (author, refined by partner): one source playbook and two
   listing files that add `tier` and `profile`; materialisation replaces
   `defaults.profile` structurally and refuses node-level profile bindings
   when `profile` is set. Alternative: two copies of the playbook, like the
   smoke playbooks. One source wins: the variants cannot drift apart.
2. Premium profile (partner): a third profile `nearbee-own-premium` pinned
   to Claude Opus on the owner's subscription; `nearbee-near` switches to
   DeepSeek V4.1 Flash first and Qwen as fallback, matching what agent A
   advertises. Haiku stays for smoke and tests.
3. Validator data (partner, from apb source): installed playbook from
   `GET /api/playbooks/{id}`, each profile from `GET /api/profiles/{name}`
   (the list route omits fallbacks and tiers), the OpenCode provider route
   from the workspace `opencode.json`, decisions and compaction from the
   project `.apb/config.yaml`. A model id prefix proves nothing about the
   destination, so the provider base URL and key reference are checked too,
   and model ownership comes from the vendored dated catalog, not a live
   call. The facts cross the `Runner` seam as typed data with explicit
   unknowns; the core validates and renders.
4. Attested subset (partner): no connectors, no profile skills,
   `environment: minimal`, no sub-playbooks, no `human_review` or
   interactive nodes, decisions disabled, context compaction disabled
   (apb's compaction calls Claude outside node profiles). Anything apb
   cannot answer refuses an attested listing.
5. Statement (both): deterministic text without timestamps or prices,
   recorded with its hash and a fingerprint of the validated facts, since a
   changed fallback can leave the prose identical. Checked at serve start
   and again before every apb start; the generation snapshots its own
   statement so recovery delivers what was promised at start.
6. Evidence (partner): fail closed. Attestation reports for every model the
   profile can use, fetched before the apb start and after the run ends,
   with bearer auth, a fresh nonce, bounded retries, raw bytes kept outside
   the worktree, labelled as collected, not verified. Start evidence is
   persisted before the start and never re-fetched to stand in for a lost
   one. apb's `attempt_models` gives the requested model per node; the
   deliverable says "requested by apb", never more.
7. Output (partner): embed the validated findings JSON in the Markdown
   deliverable and keep `files` unsupported. The file trio needs its own
   recovery design (a lost reserve response cannot be reconciled), and its
   success path was never measured live.
8. Signed step (partner): a Rust `nearbee signed-call` command with a thin
   script wrapper, exact request bytes, non-streamed, `provider_tee` only.
   Its implementation is used first in S5, so S4 writes the contract and S5
   builds it with its first real caller.
9. Repository fetch (partner): no arbitrary `git clone`. A GitHub URL is
   parsed deterministically from the brief, resolved to a commit through
   fixed GitHub API paths, downloaded as a tarball from a fixed host with
   no redirects, read member by member with compressed, expanded, file
   count and time caps, regular files only. Done by a Nearbee command the
   trusted script node calls, so it is Rust and testable.
10. Injection (partner, extended by author): the agent never runs anything
    from the repository; the repository's own agent instruction files are
    renamed on extraction so no agent CLI loads them; the agent writes only
    candidate findings, and a trusted command validates them against the
    snapshot (file exists, lines exist, excerpt matches) and renders the
    final JSON and Markdown with escaping.

## Decision

S4 ships, in this order: the variant and profile plumbing; the inspection
seam and the conservative validator; the statement with its fingerprint,
serve-time drift checks and the deliverable footer; agent A as a GitHub
snapshot audit with validated, embedded findings; endpoint attestation
evidence for the attested tier with fail-closed recovery. The signed step
moves to S5 with its contract written now; output file uploads,
decision-provider chains, connector analysis and parallel runs are
deferred. The live part of S4 is what the owner's account allows: the
premium variant runs live on the subscription; the fast variant waits for
Cloud credits (O-3); setting listings live stays the owner's act (O-6).

## Rejected

1. Two copies of the playbook per variant: the variants drift.
2. Model ownership from the `nearai/` prefix alone: proves no destination.
3. Live `GET /v1/models` at validation time: non-deterministic, and a
   network failure would block publishing; the dated snapshot is the
   contract, refreshed by hand.
4. Delivering "attestation unavailable" on an attested listing: a promise
   the deliverable does not keep.
5. Market file uploads in S4: no safe recovery for a lost reserve, success
   path unmeasured.
6. `git clone` of a buyer URL: arbitrary hosts, hooks, unbounded size.
7. Letting the agent extract the repository URL: the fetch target would be
   chosen by a model reading untrusted text.
8. Per-hire parallel runs: solve neither egress nor credential isolation;
   D-025 stays.
9. Building `nearbee signed-call` in S4: no caller until S5, and a billed
   call cannot be checked live with an empty balance.

## Open questions

None that change the specs or the tasks. The fast variant's live run waits
for the owner's Cloud top-up (O-3) and is reported as such.

## Process

Partner: Codex (GPT family), read-only on the repository and on apb's
source at `v1.0.0-beta.6`, one round (the owner asked for one round only).
The author is Claude. The partner reviewed apb source paths for profile,
connector, decision and compaction facts; the author checked the
Verification Center URL form on docs.near.ai.
