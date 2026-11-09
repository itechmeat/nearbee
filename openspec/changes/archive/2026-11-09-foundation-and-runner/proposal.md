# Proposal: foundation and runner

## Why

Stage S1 of `docs/stages.md` (plan items 3, 4 and 5). Nearbee has no code;
every later stage needs the workspace, the configuration, the two seams and
the hire journal format, and the plan puts the riskiest outside paths first:
driving apb over REST up to a validated result, and running a playbook step
on NEAR AI Cloud TEE models. See `brainstorm.md` for the chosen approach.

## What Changes

1. A cargo workspace with `crates/nearbee-core`, `crates/nearbee-market`,
   `crates/nearbee-apb`, `crates/nearbee` (binary), Rust edition 2024, and a
   GitHub Actions workflow running the native gate.
2. `nearbee-core`: domain types, the `Market` and `Runner` traits (defined
   here, implemented in the adapter crates), the result manifest v1 schema
   and validator, the hire journal record types and on-disk layout (no
   writer), configuration types and loading.
3. `nearbee-apb`: `ApbRunner`, the REST client for a local `apb dashboard`
   pinned to one apb workspace: resolve the workspace, list playbooks,
   estimate, start a run with `hire_id` and `execution_id` search params,
   read a run, find runs by search field; typed errors that separate
   refusals, retryable reads, unknown start outcomes and protocol errors.
4. `nearbee-market`: the crate exists with no implementation yet (S2).
5. `nearbee` binary: `nearbee config check` (loads the config, reports which
   required variables are present by name only, reaches the apb dashboard and
   resolves the workspace) and `nearbee runs find --hire-id <id>`.
6. `playbooks/`: profile templates `own` and `near` (OpenCode with a NEAR AI
   Cloud custom provider), a `near-vision` template marked unsupported, a
   synthetic smoke playbook that writes a valid manifest.
7. `.env.example`, `nearbee.example.yaml`.
8. Live measurements (moved to S2 on 2026-11-09, the Cloud balance being
   empty): the smoke playbook on
   `Qwen/Qwen3.8-27B` and on `deepseek-ai/DeepSeek-V4.1-Flash-1017`.
9. Document corrections from the brainstorm: `docs/apb.md` (auth header,
   run detail fields, workspace selection, budget policy, `execution_id`),
   `docs/architecture.md` (core may do narrow synchronous file checks;
   ambiguous starts are not resolved by an empty lookup).

## Capabilities

### New Capabilities

- `nearbee-config`: how Nearbee loads its configuration and secrets and how
  `config check` reports them.
- `apb-runner`: what Nearbee requires from and does with a local apb:
  workspace pinning, run start with search params, run observation, lookup,
  error classes.
- `result-manifest`: the contract a market playbook's `out/result.json`
  must meet and the checks Nearbee runs before anything is delivered.
- `hire-journal`: the record format and layout of a hire's journal.
- `executor-profiles`: the profile templates Nearbee ships and the privacy
  rules they follow.

### Modified Capabilities

None (first change).

## Impact

Crates touched: all four are created; seams hold (`nearbee-core` depends on
no adapter; adapters depend on core). New dependencies: `tokio`, `reqwest`
(rustls, no system proxy, no redirects for loopback), `serde`, `serde_json`,
`yaml_serde`, `dotenvy`, `clap`, `thiserror`, `sha2`, `tracing`,
`rust_decimal`, `uuid` (v7), `rustix` (unix, `fs`); dev:
`wiremock`, `tempfile`, `h2`. Outside systems: the local apb dashboard (read and
run), NEAR AI Cloud (two small billed runs, within the owner's USD 10
credit, O-3).

## Out of scope

Market client code and listings (S2), journal writing, locking and recovery
(S3), intake, pricing and budgets beyond passing `on_no_estimate`, attachments,
privacy-tier validation (S4), vision inference (S5), webhooks, questions and
review gates (D-011).
