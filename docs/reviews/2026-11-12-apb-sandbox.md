# Cycle report: apb-sandbox

On the owner's go-ahead of 2026-11-12 to make the agents' runs work in a
sandbox with the usual tools.

1. Artifacts: one independent review round (a fresh Claude Opus context):
   1 blocker, 9 majors, 7 minors, all folded into the design before
   implementation (`openspec/changes/archive/2026-11-12-apb-sandbox/review.md`).
2. Implementation: Claude Opus, 7 of 7 tasks; it ran its live checks on a
   separate test unit and port, never on the owner's instance. One measured
   deviation reconciled in the spec: `trust.json` and `projects.json` stay
   writable because apb replaces them by rename.
3. Native gate on the final head: `cargo fmt --all --check` clean, `cargo
   clippy --workspace --all-targets -- -D warnings` clean, `cargo test
   --workspace` 803 passed and 0 failed, `bash -n` on the changed scripts,
   `openspec validate --specs --strict`.
4. Acceptance: the integrator switched Nearbee's real instance to the
   sandbox and ran a live fast audit through it (`docs/demo-log.md`).
5. Code review: one independent Claude Opus round on the diff found 2
   high, 2 medium and 4 low findings, all fixed by the implementer in the
   same worktree, tests first:
   1. high: a symlinked protected config file could mount a host file into
      the container; protected files are now checked (regular, owned, no
      symlink) and `policy.yaml` always exists and is bound read-only;
   2. high: `install` wrote through workspace paths a step could symlink;
      in sandbox mode those writes happen inside the container with
      no-follow, exclusive create and rename;
   3. medium: install temp files sat in a step-writable directory; there
      are none now, the bundle goes over stdin and the accept digest is
      computed locally;
   4. medium: the internal bridge carried a host address; the network is
      created with the isolated gateway mode, host ports are unreachable;
   5. low: failed starts left a gateway or the env file behind; cleaned;
   6. low: variable names that collide with the sandbox environment or
      `DOCKER_` are refused;
   7. low: word lists are no longer globbed and unsafe mount paths are
      refused;
   8. low: the gateway runs as an unprivileged user with no capabilities,
      `no-new-privileges` and a read-only root.
   The gate and a second live fast audit through the sandbox passed after
   the fixes (803 tests).
6. Gaps stated plainly: every reviewer and the implementer were Claude; no
   OCR, security-audit or test-audit round ran; the subscription smoke
   inside the sandbox did not run (no Claude token is configured for the
   instance).
