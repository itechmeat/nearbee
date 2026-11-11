#!/bin/sh
# Takes the snapshot of the one public GitHub repository the run's brief
# names into audit-snapshot/ (nearbee audit fetch). Runs in the run's
# worktree; NEARBEE_BIN comes from the apb instance's environment.
set -eu
exec "${NEARBEE_BIN:-nearbee}" audit fetch
