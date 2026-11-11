#!/bin/sh
# nearbee digest extract, run in the hire's worktree. NEARBEE_BIN and
# NEARAI_API_KEY come from the apb instance's environment.
set -eu
exec "${NEARBEE_BIN:-nearbee}" digest extract
