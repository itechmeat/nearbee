#!/bin/sh
# Checks audit/candidates.json against the snapshot and writes
# out/findings.json, out/deliverable.md and the Nearbee result manifest
# out/result.json last (nearbee audit finish). Runs in the run's worktree.
set -eu
exec "${NEARBEE_BIN:-nearbee}" audit finish
