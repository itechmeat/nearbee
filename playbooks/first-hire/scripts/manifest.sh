#!/bin/sh
# Writes out/result.json (Nearbee result manifest v1) for out/deliverable.md,
# with its real size and SHA-256. Runs in the run's worktree.
set -eu
md=out/deliverable.md
[ -f "$md" ] || { echo "missing $md" >&2; exit 1; }
[ -L "$md" ] && { echo "$md is a symbolic link" >&2; exit 1; }
size=$(wc -c < "$md" | tr -d ' ')
sha=$(sha256sum "$md" | cut -d' ' -f1)
printf '{"version":1,"markdown":{"path":"deliverable.md","size":%s,"sha256":"%s"}}\n' \
  "$size" "$sha" > out/result.json
echo "manifest: deliverable.md $size bytes sha256 $sha"
