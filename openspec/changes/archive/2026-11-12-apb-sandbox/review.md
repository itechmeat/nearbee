# Independent review

## Reviewer

A fresh Claude Opus context that did not write the artifacts, read-only on
the change folder and the repository at `main`; one round, as the owner
asked for one review round only. The author checked each finding and
decided every one; there was no re-review.

## Findings

1. blocker: the publish install writes its bundle and export file to the host `/tmp`, invisible to a containerised CLI. Fixed: design 2a moves them under the config directory (a small Rust change, task 2.0).
2. major: a relative `APB_BIN` resolves against the workspace. Fixed: absolute path required (design 2.6, spec).
3. major: `stop` kills runs and a killed client leaves the container. Fixed: `docker rm -f` before run, `ExecStopPost`, `apb resume --all` after start.
4. major: the gateway resolved the dashboard by name. Fixed: fixed IP, Docker resolver, restart policy, config hash label.
5. major: OpenCode config planted in the sandbox home or the writable XDG dir escapes the tier check. Fixed: XDG read-only, an empty read-only `.opencode` bind, start refuses entries; limits documented.
6. major: the docker CLI read its config from the writable sandbox home. Fixed: `DOCKER_CONFIG` in a Nearbee-owned directory outside every mount.
7. major: the writable config directory exposed trust, policy and key files. Fixed: those files bound read-only; the exposure left is documented.
8. major: first-run order and root-owned mount directories. Fixed: mount sources created as the owner, one-off container for register and install, a fallback working directory.
9. major: API key flow gaps. Fixed: key issued only when none exists, written to Nearbee's `.env`, refused in the passed variables, header from a 0600 file.
10. minor: symlinked paths. Fixed: realpath and a workspace consistency check.
11. minor: tmpfs noexec. Fixed: `exec` on `/tmp`.
12. minor: agent CLIs need extra hosts or tokens. Fixed: documented, the subscription smoke added to the verification.
13. minor: stale binaries in the image. Fixed: sha256 labels, start refuses a mismatch.
14. minor: git identity. Fixed: the nearbee git identity passed in.
15. minor: unverifiable task wording. Fixed: environment wording, certificate fingerprint, wrapper through Nearbee, default mode check, `bash -n` instead of shellcheck.
16. minor: the CLI timeout did not reach the container. Fixed: `timeout` inside `docker exec`.
17. minor: domain fronting. Fixed: listed in the limits.

Ready for implementation: yes
