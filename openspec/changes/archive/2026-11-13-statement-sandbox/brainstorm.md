# Brainstorm

## Problem

Since `apb-sandbox` the owner's instance runs in a container, but every
generated data statement still says "The steps run without a sandbox", and
the live listing's description carries that sentence. The statement must
describe what is actually enforced, from facts Nearbee reads, not from a
flag the owner sets.

## Related work

`apb-sandbox` (the sandbox), `repo-audit-and-tiers` (the statement and the
facts fingerprint), `judges-path-and-submission` (the sandbox sentence).

## Doubts

1. Where the fact comes from: a config flag can lie; the unit's command
   line is what runs.
2. The fingerprint: adding a field must not change the fingerprint of an
   instance without a sandbox.
3. Which hosts to name: the hosts file entries of the container are the
   allowlist the gateway was generated from.

## Approaches considered

1. Config flag `apb.sandbox: true` (author): simple, but unverified.
2. Read `systemctl --user show nearbee-apb -p ExecStart` (author): the
   inspect code already reads this unit's environment the same way. The
   argv shows `docker run`, the limits and `--add-host <host>:<ip>` per
   allowed host (measured on the running unit, 2026-11-13).

## Decision

Approach 2. A sandbox fact exists only when the unit's argv is a `docker
run` of a `nearbee-sandbox` image with `--read-only`, `--cap-drop ALL`,
`--security-opt no-new-privileges`, `--dns 127.0.0.1` and a `--network`;
the allowed hosts are the `--add-host` names. Anything else, or a failed
read, means no sandbox fact and the old sentence.

## Rejected

1. Config flag: the statement would trust what the owner typed.

## Process

Author: Claude, on the owner's go-ahead of 2026-11-13 ("do everything that
is needed"). The argv format was read from the running unit.
