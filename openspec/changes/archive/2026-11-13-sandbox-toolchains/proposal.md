# Proposal: sandbox toolchains

## Why

The owner's agents (`nearbee-agents`) now include agents that build and
test a buyer's repository (`bug-repro-fix`) and upgrade its dependencies
(`vuln-deps-fix`). The sandbox has no package registries, no Go, Rust or
uv, no C toolchain, no vulnerability scanner and no `bubblewrap`, so those
hires are declined. The owner also moved the Claude calls of the script
agents from the Claude subscription to an Anthropic API key (owner
2026-11-13: within the API terms; after the hackathon the calls move to
Anthropic models through NEAR AI Cloud), and the statement must say which
account a script's CLI uses. The fast review now sends the buyer facts the
deliverable cannot carry (a failed lens, findings the verifier dropped),
and D-027 still says one signed call.

## What Changes

- The default egress allowlist adds the package registries and the OSV
  database; the statement lists them like every other host.
- The sandbox image adds `build-essential`, `bubblewrap`, Go, Rust, uv,
  `osv-scanner` and `pip-audit`, each pinned.
- A listing may say which account an agent CLI uses
  (`script_accounts: {claude: anthropic-api}`); the statement names it.
- `nearbee audit finish` reads optional review notes from the candidates
  file and renders them; D-027 and `docs/privacy.md` describe the review
  with several calls and one signed verdict.

## Capabilities

### Modified Capabilities

- `apb-sandbox`: hosts and toolchains.
- `listing-publish`: `script_accounts`.
- `data-statement`: the account wording.
- `repo-audit`: review notes.

## Impact

`sandbox/Dockerfile`, `sandbox/egress.allow`, `sandbox/build.sh` if needed;
`crates/nearbee-core/src/listing.rs`, `statement.rs`, facts and drift where
`script_models` is threaded; `crates/nearbee/src/audit.rs`; docs
(`docs/apb.md`, `docs/privacy.md`, `docs/decisions.md` D-027 amended,
`docs/architecture.md` if it lists hosts or fields). Listings without
`script_accounts` keep their statements apart from the host list.
