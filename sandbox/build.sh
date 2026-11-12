#!/usr/bin/env bash
# Builds the image of Nearbee's sandboxed apb instance.
#
#   sandbox/build.sh [tag]     builds nearbee-sandbox:<tag> (default: local)
#
# Copies into sandbox/bin/ (git-ignored, removed afterwards): apb (APB_BIN,
# else apb on PATH), nearbee (NEARBEE_BIN, else nearbee on PATH) and, when
# found, the resolved binaries of the agent CLIs named in
# NEARBEE_SANDBOX_CLIS (default: claude codex opencode). A CLI that is
# missing or is a script rather than a self-contained binary is skipped with
# a line saying so. The image is labelled with the sha256 of the copied apb
# and nearbee: a sandboxed start refuses an image whose binaries differ from
# the host's.
set -euo pipefail

TAG="${1:-local}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$HERE/bin"

die() { echo "sandbox/build: $*" >&2; exit 1; }

command -v docker >/dev/null || die "docker is not installed"
apb="${APB_BIN:-$(command -v apb || true)}"
nearbee="${NEARBEE_BIN:-$(command -v nearbee || true)}"
[ -n "$apb" ] && [ -x "$apb" ] || die "apb not found (set APB_BIN)"
[ -n "$nearbee" ] && [ -x "$nearbee" ] || die "nearbee not found (set NEARBEE_BIN)"

rm -rf "$BIN"
mkdir -p "$BIN"
trap 'rm -rf "$BIN"' EXIT

is_elf() { [ "$(head -c 4 "$1" | od -An -c | tr -d ' ')" = '177ELF' ]; }

install -m 0755 "$(readlink -f "$apb")" "$BIN/apb"
install -m 0755 "$(readlink -f "$nearbee")" "$BIN/nearbee"
for cli in ${NEARBEE_SANDBOX_CLIS:-claude codex opencode}; do
  found="$(command -v "$cli" || true)"
  if [ -z "$found" ]; then
    echo "sandbox/build: $cli not found; skipped"
    continue
  fi
  real="$(readlink -f "$found")"
  if ! is_elf "$real"; then
    echo "sandbox/build: $cli ($real) is not a self-contained binary; skipped"
    continue
  fi
  install -m 0755 "$real" "$BIN/$cli"
  echo "sandbox/build: $cli from $real"
done

sum() { sha256sum "$1" | cut -d' ' -f1; }
docker build \
  --label "nearbee.sandbox.apb-sha256=$(sum "$BIN/apb")" \
  --label "nearbee.sandbox.nearbee-sha256=$(sum "$BIN/nearbee")" \
  -t "nearbee-sandbox:$TAG" "$HERE"
echo "sandbox/build: built nearbee-sandbox:$TAG"
