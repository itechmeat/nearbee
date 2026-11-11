#!/bin/sh
# Packs the snapshot and writes audit/candidates.json from one signed
# NEAR AI Cloud call (nearbee audit review). Runs in the run's worktree;
# NEARBEE_BIN and NEARAI_API_KEY come from the apb instance's environment.
set -eu
exec "${NEARBEE_BIN:-nearbee}" audit review
