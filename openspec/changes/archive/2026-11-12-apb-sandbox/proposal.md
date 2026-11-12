# Proposal: apb sandbox

## Why

Market hires run buyer-controlled input through scripts and agent CLIs with
the owner's rights and full network access (O-11). The owner wants every
run in a container with the usual tools and an outbound allowlist.

## What Changes

- `sandbox/Dockerfile` and `sandbox/build.sh`: the image `nearbee-sandbox`
  (Ubuntu 26.04, the document tools, git, gh, ripgrep, Node with OpenSpec
  and OpenCodeReview, Code Ranker, apb and nearbee, and the self-contained
  agent CLI binaries found on the host).
- `sandbox/egress.allow`: the default outbound host allowlist.
- `scripts/apb-instance.sh`: with `NEARBEE_APB_SANDBOX=1`, `start` runs the
  instance inside the image on an internal network behind an SNI allowlist
  gateway, `stop` stops it, `down` removes the gateway and networks, and the
  `apb` subcommand and `install` go through `docker exec`.
- `scripts/apb-sandboxed`: an `APB_BIN` wrapper for Nearbee's apb CLI calls.
- `crates/nearbee-apb/src/install.rs`: the bundle goes to apb on stdin and the export read-back comes from stdout, so no temporary file exists.
- Docs: `docs/apb.md`, `docs/privacy.md`, `README.md`.

## Capabilities

### New Capabilities

- `apb-sandbox`: running Nearbee's apb instance in a container with an
  outbound allowlist.

### Modified Capabilities

None.

## Impact

Shell scripts, docs and a small Rust change (the publish install passes the bundle on stdin and reads the export from stdout). Needs Docker. The default
mode (no sandbox) is unchanged.
