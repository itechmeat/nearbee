# Brainstorm

## Problem

Nearbee has no code yet. Stage S1 (`docs/stages.md`) must lay the cargo
workspace, the configuration, the seams (`Market`, `Runner`) and the hire
journal format, and it must prove the two riskiest outside paths: driving a
local apb over its REST API from start to a validated result manifest, and
running a playbook step on NEAR AI Cloud Confidential TEE models. Everything
later (market, durable hires, example agents) builds on these types and on
what this stage measures.

## Related work

1. `docs/decisions.md`: D-003 (Rust), D-004 (apb over REST), D-012 (buyer
   data kept out of apb learning features), D-013 (materialised variants),
   D-014 (result manifest), D-015 (journal with intents, `hire_id` search
   param), D-021, D-022.
2. apb's own OpenSpec change `near-ai-cloud-models` (proposed 2026-11-09, not
   released): NEAR models through apb's built-in `dsh` agent with a
   run-scoped route, key `NEARAI_API_KEY`, privacy class per model, refusal
   `cloud_model_no_tools` for models without tool calls.
3. No open pull requests, no archived changes; this is the first change.

## Doubts

The author put eight doubts to the partner: project selection and worktree
handling on the REST start, the exact run detail fields and terminal states,
budget refusals, dashboard authentication, the error taxonomy and ambiguous
starts, the minimal journal record set, scope for one day, crate choices.

## Approaches considered

1. **Traits** (author: generics with native async fn; partner: same, but the
   traits and their domain types live in `nearbee-core` and declare
   `-> impl Future<Output = ...> + Send` so later spawning works). Adopted
   with the partner's refinement; `async_trait` and `dyn` rejected.
2. **Core purity** (author: journal and manifest in core with `std::fs`;
   partner: only narrow synchronous filesystem helpers for manifest
   validation, journal writing is S3, and the "no I/O" statement in the
   architecture is revised explicitly). Adopted the partner's version.
3. **Manifest** (both): core owns the schema and the checks; version 1 is
   frozen now with paths relative to `out/`, byte sizes and SHA-256 per
   entry, public and private evidence kept apart. Partner added: reject a
   symlinked or escaping `out/`, non-regular files, unbounded reads; keep the
   validated bytes for submission instead of reopening paths; check the final
   assembled Markdown against 262,144 UTF-8 bytes.
4. **Fixtures** (author: env-flag recording; partner: an explicit capture
   helper, never a test that overwrites fixtures, replay through `wiremock`
   matching method, path, query and body; fixtures carry status, headers, apb
   version and date, scrubbed). Adopted the partner's version.
5. **Config format** (author leaned to TOML; partner: keep YAML with
   `yaml_serde`, which apb itself uses, since listings, profiles and
   playbooks are YAML). Adopted YAML. Precedence: defaults, file, `.env`,
   process environment (process wins).
6. **Cloud path** (both): the installed apb's OpenCode custom provider today,
   profile names stable so the later switch to apb's `dsh` route is a profile
   edit; `NEARAI_API_KEY` must be allowed into the executor environment and be
   present in the environment of the running apb dashboard. Partner found
   that a vision template cannot work as an agent profile (Qwen3-VL has no
   tool calls; apb's own change refuses it), so `near-vision` ships marked
   unsupported in S1 and its design moves to S5.
7. **Run lookup** (partner, from apb source): REST is enough,
   `GET /api/runs?workspace=&playbook=&field=hire_id:<id>`; the filtered
   answer is `{runs, not_indexed, withheld}`, so a lookup returns matches plus
   completeness, never a bare "none".

## Decision

S1 builds four crates with the traits and domain types in `nearbee-core`;
six runner operations (projects, playbooks, estimate, start, run detail,
lookup) against apb pinned to one workspace; a strict manifest v1 validator
in core; YAML config with environment overrides; the journal envelope and
intent/result record types (no writer); OpenCode-based `own` and `near`
profile templates; one synthetic smoke playbook run live on the two text
models. Findings that correct `docs/apb.md` and `docs/architecture.md` land
in the same change:

1. `X-Requested-With: apb-dashboard` is a CSRF marker for cookie sessions,
   not an alternative credential; a keyless loopback dashboard needs no
   header, a keyed one needs `Authorization: Bearer`.
2. `RunDetail` has `run_status` (`created`, `running`, `paused`,
   `succeeded`, `failed`, `aborted`, `interrupted`; only `succeeded`,
   `failed` and `aborted` are terminal), `nodes` and `outputs` maps, optional `worktree` and
   `usage`; there is no `outputs.files` field (that is a node declaration;
   captured files appear as artifact references), so the manifest is the only
   authority on results.
3. The start selects the project with `?workspace=<id>`, takes
   `params: map<string,string>`, refuses a worktree that does not exist or is
   not inside the project or a git worktree of it (422); subdirectories share
   the project's workdir lock, so S1 runs serially and S3 proves concurrency
   with git worktrees.
4. Budget: Nearbee passes `on_no_estimate: continue` explicitly (pricing
   doc), the "pause once estimates exist" line in `docs/apb.md` is removed;
   `budget_needs_decision` is a typed refusal, never auto-answered.
5. Every run carries a second search param `execution_id` besides
   `hire_id`, generated and journaled before the start, so rework runs of one
   hire and duplicate starts of one execution are told apart.
6. An ambiguous start (timeout, reset, malformed success) is never replayed;
   lookup by `execution_id` first, and an empty lookup does not prove that
   nothing is being prepared: the hire stays ambiguous for bounded
   reconciliation (S3), as `docs/architecture.md` will now say.

## Rejected

1. `async_trait` with `Arc<dyn ...>`: one implementation per seam in v1.
2. TOML config: a second format beside YAML listings and profiles.
3. A general `Fs` or clock trait in S1: no consumer yet.
4. Recording fixtures from tests behind an env flag: overwrites by accident.
5. A working `near-vision` agent profile in S1: the model has no tool calls.
6. A journal writer in S1: durability, locking and replay belong to S3.
7. A retry framework or an axum/hyper mock server in production code.

## Open questions

1. **D-012 cannot be enforced today** (partner, from apb source): apb runs
   corrections and known-problem observation automatically at run end, has
   no project setting that turns them off, and can recall omitted params from
   memory even with `remember: false`. Resolved by the owner on 2026-11-09
   (D-023): an external-data run flag is requested from apb in the same
   change as NEAR AI support; S1 runs synthetic data only, so it is not
   blocked.

## Process

Partner: Codex (a different model family from the author), read-only on the
repository and the apb source, one round. Its sandbox forbade sockets, so
live dashboard behaviour is to be confirmed by the implementer's fixture
capture.
