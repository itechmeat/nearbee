# Design: sandbox toolchains

## 1. Egress hosts

Add to `sandbox/egress.allow`, each with a comment saying who needs it:
`registry.npmjs.org`, `registry.yarnpkg.com`, `repo.yarnpkg.com` (Yarn's
default registry and the Yarn releases corepack fetches); `pypi.org`,
`files.pythonhosted.org`;
`proxy.golang.org`, `sum.golang.org`; `index.crates.io`,
`static.crates.io`; `osv-vulnerabilities.storage.googleapis.com`,
`api.osv.dev`. Nothing else. The statement's "Where the run happens" list
is generated from this file, so it changes for every listing; the owner
re-pastes the statement once (the drift check already stops a listing
whose facts changed).

## 2. Image

In `sandbox/Dockerfile`, pinned the way Node is (version plus SHA-256 of
the official artifact, verified at build):
1. apt: `build-essential`, `bubblewrap`, `pkg-config`.
2. Go: the official `go<ver>.linux-amd64.tar.gz` from go.dev, newest
   stable, into `/usr/local/go`, on `PATH`; `GOTOOLCHAIN=local`.
3. Rust: rustup with the minimal profile and a pinned stable toolchain
   at build time (build hosts are not runtime egress), `CARGO_HOME` and
   `RUSTUP_HOME` read-only in the image, `PATH` set;
   `RUSTUP_TOOLCHAIN` set to the pinned toolchain, so a repository's
   `rust-toolchain.toml` does not start a download the sandbox cannot make
   (the counterpart of `GOTOOLCHAIN=local`).
4. uv: the pinned static release binary.
5. `osv-scanner`: the pinned release binary (2.x).
6. `pip-audit`: in its own venv, its entry point on `PATH`, installed
   with `--require-hashes` from `sandbox/pip-audit.lock`, which pins it and
   every dependency by version and SHA-256.
Check versions are current stable (newest stable rule) and record them in
`docs/apb.md`. The image size grows; record the measured size.

`bubblewrap` may not start inside the container without added privileges;
do not add privileges. Probe it in the built image
(`bwrap --ro-bind / / true`) and record the result in `docs/apb.md`; the
agent decides from that.

## 3. Script accounts

Listing field `script_accounts`: a map from an agent CLI named in
`script_models` to an account kind. Closed values: for `claude`,
`subscription` (the default when absent) and `anthropic-api`. A key not
used by `script_models`, an unknown agent key or an unknown value is
refused naming it. Recorded and frozen with the listing like
`script_models`; facts gain it with `skip_serializing_if` empty, so
listings without it keep their fingerprints.

Statement wording for `claude` with `anthropic-api`:
"- `claude`, model `<model>`: the owner's Anthropic API account (API key)."
The subscription wording stays as it is.

## 4. Review notes in the fast deliverable

`audit/candidates.json` version 1 gains an optional `notes` field: at most
10 strings, each at most 300 characters, rendered by `audit finish` as a
`## Review notes` section before "Not reported", escaped like other
untrusted text. Absent or empty: the deliverable is byte for byte as
before. `deny_unknown_fields` stays for every other field.

## 5. Docs

`docs/apb.md`: the new hosts and tools with versions, the bwrap probe
result, `ANTHROPIC_API_KEY` in the `NEARBEE_APB_ENV_VARS` example.
`docs/privacy.md`: `script_accounts` and its wording. `docs/decisions.md`:
amend D-027 (the fast review makes several direct calls, lenses on
DeepSeek V4.1 Flash and a verdict on Qwen3.8 27B, and signs the verdict
call; date 2026-11-13).

## 6. Amendment: deliver output files (live hire 2026-11-13)

A paid `ux-flow` hire produced a good `deliverable.md` plus `flow.json`
and was declined `failed` because serve refuses a manifest that lists
output files ("not supported yet"). Every agent of the owner lists files
(flow.json, patches, locale files, JSON data), so serve SHALL deliver them:

1. For each `files[]` entry of a valid manifest (path beneath `out/`,
   size and SHA-256 rechecked as today for the Markdown): reserve
   `POST /v1/files {filename, byteSize, link: true}`, `PUT` the bytes to
   the returned upload URL, `POST /v1/files/{id}/complete`. When the
   complete answers `200`, check its `byteSize` and `sha256` against the
   manifest (a difference declines and deletes the file). When it answers
   `409 upload_not_pending` (the live behaviour: the same-origin sink
   stores the bytes on the PUT) the market reports no digest, and the
   API has no read of a file's `byteSize` or `sha256` (the OpenAPI has
   only the delete, the raw download and the link download for a file);
   the check is then that the PUT of the exact bytes succeeded against an
   upload URL signed for the reserved `byteSize`, and the digest serve
   checked locally before the upload is the manifest's. Only the code
   `upload_not_pending` counts as done; any other `409` is a refusal.
   Then submit
   `{deliverableMarkdown, files: [fileUrl ...]}` (docs/market.md: a
   `fileUrl`, bare token or `fileId` each).
2. Limits by code before any upload: at most 20 files, each at most
   10 MiB, 50 MiB in all, names `[A-Za-z0-9._-]` after the last `/`
   (a name clash gets a numeric suffix); a manifest past a limit is
   declined `failed` with the limit named. The secret scan runs on every
   text file as it does on the Markdown; a hit declines.
3. Journal and idempotency like every market write: an intent per file
   reserve (operation id, file digest), closed with the reserved file id;
   then an intent for the PUT and complete naming that id, closed with the
   link. A restart resumes from the journal and never uploads a completed
   file twice; an unbound reserved file left by a failure is deleted
   (`DELETE /v1/files/{id}`, docs/market.md), including one a crash left
   after its reserve (the open complete intent is closed `lost` and its
   file id deleted). Every deletion serve makes is journaled (a `deleted`
   result on the upload's operation) before the `DELETE`, and a deleted
   upload is never reused. A reserve whose answer was lost leaves a slot
   only the market's 7-day sweep removes. A `408`, `425` or `429` answer
   leaves the upload pending like an unknown outcome (one of the three
   attempts); any other refusal declines the hire.
4. The deliverable Markdown gets a short "Files" list naming each
   delivered file with its size, so the buyer sees them.
5. Contract tests against recorded responses shaped by the OpenAPI
   (memory: market mocks from OpenAPI); read the vendored worker skill and
   `docs/market.md` for the exact fields; record any measured fact in
   `docs/market.md`.
