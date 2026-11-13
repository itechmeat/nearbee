# Review: sandbox-toolchains (uncommitted diff + openspec/changes/sandbox-toolchains)

Scope: `git diff` on branch `sandbox-toolchains` plus the untracked change
folder, read against proposal.md, design.md and the five spec deltas.
Read-only. Focus: output-file upload in serve, `script_accounts`, audit
notes, Dockerfile pins. Seven findings, most severe first.

## 1. A 429 (or any 4xx) on an upload write declines a finished paid hire (medium-high)

`crates/nearbee-core/src/serve.rs:4382`. `write_once`
(`crates/nearbee-market/src/lib.rs:559`) maps every non-401 4xx, 429
included, to `MarketError::Refused`. `upload_outputs` treats every
`Refused` as final: it journals `refused`, deletes the earlier uploads and
returns `Uploads::Refused`, and `finish_terminal` opens a `failed` decline.
A transient rate limit therefore costs a successful paid run (a refund and
a hit to the success rate). Uploads multiply the risk: up to 20 files x 2
writes each (reserve and complete) in one burst, against limits that
`docs/market.md` "Not yet checked" item 2 says are unknown. Example: a
`ux-flow` run lists 8 files; the market answers `429` on the 6th reserve;
the hire is declined `failed` with "the result files could not be
delivered". Fix: treat 429, and other retryable codes such as 408 and 425,
as a pending outcome (`Uploads::Pending`, counted against
`UPLOAD_ATTEMPTS`), not as a refusal.

## 2. In practice the size and digest check never runs (medium; spec deviation)

`crates/nearbee-core/src/serve.rs:4349` with
`crates/nearbee-market/src/lib.rs:1233`. The check runs only when
`up.stored` is `Some`. `docs/market.md` (Output file delivery 1) records
that the live market stores the bytes on the PUT, so the complete
normally answers `409 upload_not_pending` and `stored` is `None`. The
normal path never compares the stored SHA-256 with the manifest. Design
6.1 and the hire-serving scenario ("uploads `flow.json`, checks its size
and digest") require the check. The client also accepts the 409 by its
code alone, so a 409 for an upload in another non-pending state (failed or
expired, if the market reuses the code) would also count as delivered with
no verification. Fix: when the complete gives no digest, read the file's
metadata (or HEAD the `fileUrl`) and check `byteSize` and `sha256`, or
amend the spec and design to say the check is the signed byte count only.

## 3. A Rust repo with `rust-toolchain.toml` cannot build in the sandbox (medium)

`sandbox/Dockerfile:71` and `:96`. Go is held to the image's toolchain
with `GOTOOLCHAIN=local`, but Rust has no equivalent. rustup 1.29
auto-installs the toolchain a project's `rust-toolchain.toml` or
`rust-toolchain` file names. That install fails in the sandbox:
`static.rust-lang.org` is not in egress, and `RUSTUP_HOME` is read-only
(`chmod go-w`). Many Rust repos pin a channel. Example: a
`bug-repro-fix` hire on a repo with `channel = "1.88"` gets "could not
download" or "permission denied" on the first `cargo test`, and the step
fails. Fix: set `RUSTUP_TOOLCHAIN=${RUST_VERSION}` (it overrides toolchain
files) or `RUSTUP_AUTO_INSTALL=0` with a clear message, and record this in
`docs/apb.md`.

## 4. Uploads are deleted before the journal says so; a crash then submits dead links (low-medium)

`crates/nearbee-core/src/serve.rs:4360-4364` (mismatch), `:4120`,
`:4130` and the attempt-cap and `Refused` branches. `delete_uploads` runs
before anything durable records the deletion or the decline:
`open_decline_intent` comes later, in `refuse`. Earlier files keep their
`uploaded` results. If serve crashes between the delete and the decline
intent, the next round finds the generation still `RunFinished`.
`completed_uploads` returns the deleted files as done, and they are
reused without a new upload. Example: A is uploaded, then B gets a stored
digest mismatch. A is deleted, then a crash. On restart B uploads cleanly
this time, and serve submits the Markdown with A's link, which no longer
works. The market either refuses the submit (decline) or stores a version
with a broken file. Fix: journal a `deleted` result for each file id, or
open the decline intent, before calling `DELETE`, and leave out deleted
ids in `completed_uploads`.

## 5. A crash during an upload leaves a public-link file that is never deleted (low)

`crates/nearbee-core/src/serve.rs:4252`. The intent is written before
`upload_file`, but the file id becomes known only when the whole trio
returns. A crash after the market finished the complete but before the
`uploaded` result is closed as `lost` with no file id. The file is then
uploaded again, and the first copy stays reserved and reachable by its
link until the market's 7-day expiry. Design 6.3 says "an unbound
reserved file left by a failure is deleted". The same happens in the
client when a linked reserve answers without `fileUrl`
(`crates/nearbee-market/src/lib.rs:1133`): `file_id` is known, but the
client returns `Protocol` without deleting it. A resent reserve after an
unknown outcome can also leave a second reservation. Fix: journal the
file id after the reserve (split the seam into reserve and put/complete
steps), so `lost` can delete it. Delete in the missing-`fileUrl` branch.

## 6. pip-audit is pinned by version only; its dependency tree floats (low-medium, supply chain)

`sandbox/Dockerfile:93`. Design 2 says each tool is pinned "the way Node
is (version plus SHA-256 of the official artifact, verified at build)".
`pip install pip-audit==2.10.1` checks no hash, and its roughly 20
transitive dependencies (requests, pip-api, cyclonedx, etc.) resolve to
whatever is newest at build time. A compromised or broken release of any
of them enters the image without notice, and two builds of the same
Dockerfile can differ. Fix: a hashed requirements lock
(`pip install --require-hashes -r pip-audit.lock`).

## 7. The yarn registry is missing from egress, though the comment says yarn works (low)

`sandbox/egress.allow:20` (comment on line 19: "npm (npm, pnpm, yarn)").
Yarn classic and Yarn Berry both default to `registry.yarnpkg.com`, and
corepack fetches Berry from `repo.yarnpkg.com`. Neither host is listed, so
`yarn install` on a buyer repo fails with no route, and the statement's
host list is then the only hint. Fix: add `registry.yarnpkg.com` (and
`repo.yarnpkg.com` if corepack-managed yarn is meant to work), or drop
"yarn" from the comment and document the limitation in `docs/apb.md`.

## Checked and found sound

1. `script_accounts`: closed values, unknown agent, unused key and unknown
   kind refused naming them. Facts drop default kinds, so a listing with
   no field or only `subscription` keeps its fingerprint and statement
   (tests pin both). The drift pass and both publish passes thread the
   field. The statement snapshot freezes it.
2. Audit notes: limits are checked before any output is written,
   `deny_unknown_fields` is kept, line breaks become spaces, and the text
   goes through the same `escape` as other untrusted text. It is placed
   before "Not reported", and absent notes add nothing.
3. Upload journal: records are generation-scoped (`gen_records`), so a
   rework generation uploads its files again. An open upload intent does
   not change the derived `GenState`. The attempt count does not double
   count `lost`. The file references are snapshotted with a digest in the
   submit intent and checked on resend. A refused submit deletes only the
   files of its own frozen list.
4. Limits (20 files, 10 MiB, 50 MiB, name characters) and the secret scan
   run before any upload. After the upload the deliverable is assembled
   again from a fresh read, and `same_outputs` guards against changed
   bytes. The Files list is subject to the Markdown limit and the secret
   scan.
5. Go, rustup, uv and osv-scanner downloads are checked against their
   SHA-256 at build. I did not verify the hash values themselves (task
   3.2 says the build passed).

## Summary

The change mostly matches its proposal, design and specs. The
`script_accounts` field and the review notes are correct, and the
fingerprint is stable. The issues are in output-file delivery and in the
image. Most severe: a 429 or other transient 4xx on any file upload
declines a finished paid hire, and the size and digest check the spec
requires does not run on the market's normal 409 complete path. Next: a
Rust repo that pins its toolchain cannot build, because rustup tries a
blocked, read-only auto-install. Lower: deletes happen before the journal
records them (after a crash, dead links can be submitted); a crash during
an upload leaves a public-link file behind; pip-audit's dependencies are
not hash-pinned; and the egress comment says yarn works, but its registry
is not allowed.
