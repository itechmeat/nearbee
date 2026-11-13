# Brainstorm

## Problem

`nearbee list` bundles only the regular files directly under the source
playbook's `scripts/` (`publish.rs`, `read_dir` without recursion). apb
bundles and runs every regular file under `scripts/`, subdirectories
included. A playbook whose scripts keep their library, prompts and data in
subdirectories (the owner's `ux-flow` agent: `scripts/lib`,
`scripts/prompts`, `scripts/context`) would be published without them and
fail at its first step.

## Related work

`listing-publish` (materialisation, the derived version); apb's bundle
format 2 (paths relative to `scripts/`, refused symlinks).

## Doubts

1. The derived version must not change for a flat `scripts/` directory, or
   every published listing drifts.
2. Symlinks and special files must be refused, not followed.
3. Path form: forward slashes, relative to `scripts/`, sorted.

## Approaches considered

1. Recursive walk with relative paths (author): matches apb's own bundle
   layout; a flat directory yields the same names as today.
2. Ask agents to keep `scripts/` flat: rejected, it forces a single-file
   layout on every agent.

## Decision

Approach 1. Walk `scripts/` recursively, sorted by relative path; refuse a
symlink anywhere under it with a message naming the path; skip nothing
else. The name of a top-level file stays the bare file name, so the derived
version and digest of an existing flat listing are unchanged.

## Rejected

1. Flat-only scripts: needless restriction.

## Process

Author: Claude, on the owner's go-ahead to build the first real agent
autonomously (2026-11-13).
