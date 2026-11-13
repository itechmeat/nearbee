# Implementer report: sandbox-toolchains

Date: 2026-11-13. Implementer: Claude Opus 5.5 (headless session), working
in the main checkout on branch `sandbox-toolchains` as briefed. Nothing was
committed or pushed.

## Tasks

All eight tasks of `tasks.md` are done and ticked.

1. 1.1 and 1.2, script accounts. Tests first in
   `crates/nearbee-core/src/listing.rs` (`script_accounts_are_kept`,
   `a_bad_script_account_is_refused_naming_it`),
   `crates/nearbee-core/src/statement.rs`
   (`a_claude_script_model_on_the_api_account_names_the_api_key`,
   `a_listing_without_script_accounts_keeps_its_statement_and_fingerprint`,
   `the_drift_pass_adds_the_records_script_accounts`) and
   `crates/nearbee/tests/publish.rs`
   (`a_plain_publish_records_the_script_account`). The core tests failed
   to compile before the type existed (not on an assertion); the publish
   test was checked to fail on an assertion with the publish threading
   reverted and to pass with it. Implementation: `ScriptAccount`
   (`subscription`, `anthropic-api`, kebab-case) and
   `Listing.script_accounts`/`ResolvedListing.script_accounts`
   (`BTreeMap`, `skip_serializing_if` empty) in `listing.rs`;
   `check_script_accounts` refuses an agent without kinds, an unknown kind
   and a key no `script_models` entry uses, each naming
   `script_accounts` and the value; `ExecutionFacts.script_accounts` with
   `with_script_accounts`, which keeps only non-default kinds, so a
   listing without the field or with `claude: subscription` keeps its
   fingerprint (pinned by the existing `PLAIN_PREMIUM_FINGERPRINT` and
   `PLAIN_PREMIUM_TEXT_SHA256`); the statement's script section uses the
   API wording for `claude` with `anthropic-api`; threaded through
   publish (source and installed facts, the record), the drift pass and
   serve's statement snapshot.
2. 2.1 and 2.2, review notes. Tests first in `crates/nearbee/tests/audit.rs`
   (`absent_or_empty_notes_leave_the_deliverable_unchanged`,
   `notes_are_rendered_in_order_and_escaped`,
   `too_many_or_too_long_notes_fail_with_no_result`), all three failing on
   assertions before the code; the SHA-256 of the deliverable without
   notes (`a230902e...`) was taken from the unchanged code. Implementation
   in `crates/nearbee/src/audit.rs`: optional `notes` (serde default,
   `deny_unknown_fields` kept), at most `MAX_NOTES` 10 and
   `MAX_NOTE_CHARS` 300 characters (counted in chars), checked before any
   output is written; rendered under `## Review notes` before
   `## Not reported`, one list item per note, line breaks turned into
   spaces, text through `escape`.
3. 3.1 `sandbox/egress.allow`: the nine hosts of design 1, each group with
   a comment. No test pins the file's hosts (the statement tests use
   synthetic host lists), so no test changed.
4. 3.2 `sandbox/Dockerfile` per design 2; built and probed (below).
5. 3.3 Docs: `docs/apb.md` (toolchains with versions, measured size, the
   bwrap probe, the new hosts as item 7, item 5 updated since
   `registry.npmjs.org` is now default, `ANTHROPIC_API_KEY` in the
   `NEARBEE_APB_ENV_VARS` example), `docs/privacy.md` (script accounts as
   item 4 of Script models, review notes in the fast-review section),
   `docs/decisions.md` (D-027 amended 2026-11-13),
   `docs/architecture.md` (the listing field list).
6. 3.4 Native gate: green, tails below.

## Dependency versions (newest stable on 2026-11-13)

1. Go 1.27.4 (`go1.27.4.linux-amd64.tar.gz`, SHA-256 from go.dev's JSON
   index).
2. rustup 1.29.3 (`rustup-init`, SHA-256 from static.rust-lang.org),
   toolchain 1.100.0 (minimal profile). The workspace itself stays on
   `rust-toolchain.toml` 1.99.1; only the image has 1.100.0.
3. uv 0.13.8 (`uv-x86_64-unknown-linux-musl.tar.gz`, SHA-256 from the
   release's `.sha256`).
4. osv-scanner 2.7.1 (`osv-scanner_linux_amd64`, SHA-256 from
   `osv-scanner_SHA256SUMS`).
5. pip-audit 2.10.1 (pip, version pinned like code-ranker; its own
   dependencies are resolved at build).
6. apt (Ubuntu 26.04, unpinned like the other apt packages):
   build-essential 12.12ubuntu2.26.04.2 (GCC 15.2.0), bubblewrap
   0.11.1-1ubuntu0.3, pkg-config 2.5.1-4.

## Decisions

1. Default account kinds never enter the facts, so `claude: subscription`
   written out keeps the fingerprint of a listing without the field.
2. Order of refusals: unknown agent, then unknown kind, then unused key.
3. `script_accounts` is also frozen in serve's statement snapshot (design
   3, "frozen with the listing like script_models"); it is not sent to
   the runner, since no script reads it.
4. A note's line breaks become spaces so a note cannot open a block of
   its own; `\r\n` counts as one break.
5. `CARGO_HOME` and `RUSTUP_HOME` are image paths, readable by all and
   writable by none but root; the container is read-only anyway, so a
   step that fetches crates sets `CARGO_HOME` itself (documented).

## Deviations and findings

1. The bwrap probe fails, as the design anticipated: `bwrap --ro-bind / /
   true` (also with `--unshare-user` and `--unshare-all`) prints "No
   permissions to create a new namespace" both as root without flags and
   as uid 1000 with `--read-only --cap-drop ALL --security-opt
   no-new-privileges`. Host: `kernel.unprivileged_userns_clone = 1`,
   `kernel.apparmor_restrict_unprivileged_userns = 1`. No privilege was
   added; recorded in `docs/apb.md`.
2. The spec scenario "Registries reachable, others not" was not run live:
   it needs the instance restarted with the new allowlist, which the task
   forbids. The integrator runs it at acceptance.
3. The image grew from 3.9 GB to 5.76 GB.
4. A working mistake, corrected: one early `cargo test` ran without
   `CARGO_TARGET_DIR` (the export did not persist between shell calls)
   and created `target/` in the checkout; it was removed at once (created
   that minute, git-ignored) and every later command set the variable.
5. `docs/privacy.md` still says, for the repository's own
   `playbook.fast.yaml`, that the fast listing makes one signed call: that
   is true of this repository's playbook. The several-call review is the
   owner's agents' (`nearbee-agents`), described in the new "Review
   notes" paragraph and the D-027 amendment as the design words it.

## Live run

1. `NEARBEE_BIN=$HOME/.local/share/nearbee/bin/nearbee sandbox/build.sh`:
   exit 0, `codex` 0.166.0 and `opencode` copied, `built
   nearbee-sandbox:local` (image `6d55d104eeba`, 5.76 GB).
2. In `nearbee-sandbox:local` (`--read-only --tmpfs /tmp`):

   ```
   go version go1.27.4 linux/amd64
   cargo 1.100.0 (3acc79c0c 2026-11-05)
   rustc 1.100.0 (0ef1bc386 2026-11-09)
   uv 0.13.8 (x86_64-unknown-linux-musl)
   osv-scanner version: 2.7.1
   pip-audit 2.10.1
   cc (Ubuntu 15.2.0-16ubuntu1) 15.2.0
   bubblewrap 0.11.1
   GOTOOLCHAIN=local
   ```
3. With the instance's flags (`--cap-drop ALL`, `no-new-privileges`, uid
   1000, `/tmp` exec as in `scripts/apb-sandbox.sh`): `cargo new` plus an
   offline `cargo build` ran ("Hello, world!"), `go run` and a `cc`
   compile ran.
4. The running instance was not restarted: `nearbee-apb-sbx` still runs
   the previous image `db2854c33446` (now untagged), `nearbee-apb-gw`
   unchanged. The next `scripts/apb-instance.sh start` picks the new
   image; its apb and nearbee labels match the host binaries used.

## Processes and temporary data

No process was left running. Removed at the end:
`~/.cache/nearbee-sandbox-toolchains/` (gate logs, build log) and the
helper script `~/.cache/nearbee-sandbox-toolchains-fix.py`. Kept:
`~/.cache/nearbee-sandbox-toolchains-target` (the build output, for the
integrator's reruns) and the old image `db2854c33446`, which the running
container uses.

## Files changed

`crates/nearbee-core/src/{listing,statement,tiers,serve}.rs`,
`crates/nearbee-core/tests/serve.rs`, `crates/nearbee/src/{audit,publish}.rs`,
`crates/nearbee/src/replay/{mod,runner}.rs`,
`crates/nearbee/tests/{audit,publish,doc_digest_playbook,repo_audit_playbook}.rs`,
`crates/nearbee-apb/src/inspect.rs` (new field in struct literals only),
`sandbox/Dockerfile`, `sandbox/egress.allow`, `docs/apb.md`,
`docs/privacy.md`, `docs/decisions.md`, `docs/architecture.md`,
`openspec/changes/sandbox-toolchains/tasks.md`, this report. 20 tracked
files, 591 insertions, 15 deletions. `cargo test --workspace`: 841
passed, 0 failed, 5 ignored.

## Gate tails (verbatim)

```
$ cargo fmt --all --check
fmt exit=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Checking nearbee-cloud v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee-cloud)
    Checking nearbee v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.40s
clippy exit=0

$ cargo test --workspace
   Doc-tests nearbee_market_fixture

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

test exit=0

$ openspec validate sandbox-toolchains --strict
Change 'sandbox-toolchains' is valid
validate exit=0
```


## Amendment: output files (section 4, design 6)

Date: 2026-11-13. Same implementer, same checkout and branch; nothing
committed or pushed.

### Tasks

All three tasks of section 4 are done and ticked.

1. 4.1, tests first. Serve (`crates/nearbee-core/tests/serve.rs`, the
   fake market gained `upload_file` with a durable-intent check, a
   scripted answer queue and `delete_file`):
   `output_files_are_uploaded_and_submitted_with_their_references`
   (replaces the old C11 test that expected a decline),
   `clashing_output_file_names_get_a_numeric_suffix`,
   `a_restart_after_an_upload_never_uploads_a_completed_file_twice`,
   `a_recovered_submit_resends_the_snapshotted_file_references`,
   `twenty_one_output_files_decline_naming_the_limit`,
   `output_files_over_the_size_limits_decline_before_any_upload`,
   `an_output_file_name_outside_the_allowed_characters_declines`,
   `a_secret_in_an_output_file_declines`,
   `a_stored_digest_mismatch_declines_and_deletes_the_upload`,
   `a_refused_submit_deletes_its_uploaded_files`. Market client
   (`crates/nearbee-market/tests/client.rs`): the trio test now checks the
   stored size and digest, plus
   `a_complete_answering_upload_not_pending_counts_as_done`,
   `a_failed_put_deletes_the_reserved_file`,
   `a_refused_complete_deletes_the_reserved_file`, `delete_file_answers`;
   contract (`tests/contract.rs`): the `DELETE /v1/files/{file_id}` row
   and `the_complete_answer_parses_with_its_size_and_digest`. Only the seam
   types were added first (`StoredFile`, `UploadedFile.stored`,
   `Market::delete_file`, client stub), so every new test compiled and
   failed at run time before 4.2: 10 serve tests and 5 client tests
   failed on assertions or unwraps.
2. 4.2, implementation behind the `Market` seam.
   1. `nearbee-core/src/market.rs`: `UploadedFile.stored:
      Option<StoredFile>` (size and SHA-256 from the complete answer) and
      `Market::delete_file`.
   2. `nearbee-market`: `upload_file` parses `CompleteFileResponse` (new
      wire type), treats `409 upload_not_pending` as done with no digest,
      and deletes the reserved file when the PUT or the complete fails;
      `delete_file` sends `DELETE /v1/files/{id}`, 204 and 404 count as
      gone, `409 file_bound` is a refusal.
   3. `serve.rs`: the old "not supported" refusal and its
      `unsupported_output` attention are gone. `plan_outputs` checks at
      most 20 files, 10 MiB each, 50 MiB in all, names
      `[A-Za-z0-9._-]` after the last `/`, and the secret scan on every
      file, all before any upload; clashing names get `-2`, `-3` before
      the extension. `upload_outputs` journals a `market_upload` intent
      per file (digest, path, name, size) before the call and a result
      (`uploaded` with `file_id` and `file_url`, or `mismatch`,
      `refused`, `unknown`, `lost`); a file with an `uploaded` result in
      the generation is never uploaded again; an intent a crash left open
      is closed as `lost`; three answerless uploads of one file decline.
      After the uploads the deliverable is validated again with a
      `## Files` list right after the playbook Markdown (each file linked
      as ``[`name`](<fileUrl>), N bytes``), the reread files must match
      the uploaded digests, and the submit intent freezes the links in
      `gen-<n>/submission-files.json` (ref `files_sha256`; a submission
      without files writes nothing new, so old intents and deliverables
      are byte for byte as before). `send_submit` resends the frozen links
      and, on a refused submit, deletes the uploads before the decline.
      A stored size or digest that differs from the manifest, a refused
      upload, a final-check failure after the uploads or a changed file
      deletes the uploads done so far and declines `failed` with the
      buyer text "the result files could not be delivered" (secret: the
      existing local safety check text; limits and names: "the result was
      invalid").
   4. `replay/market.rs`: `delete_file` stub like the other unused calls.
3. 4.3, docs: `docs/market.md` (the `DELETE` row, a new section "Output
   file delivery" with the 409 behaviour, the 5-file cap and why the
   linked Files list keeps every file outside it, rework re-uploads,
   `invalid_file_type`; "Not yet checked" item 4 rewritten),
   `docs/architecture.md` (advance step, the intent list, the
   reconciliation table rows for uploads and a refused submit),
   `docs/privacy.md` (section 4 of the statement list now matches the
   statement's actual text; a new "Output files" paragraph: public links,
   the scan, the limits, deletion of unused uploads, what the journal
   keeps). Native gate green, tails below.

### Decisions and deviations

1. The market caps a submit at 5 `files` entries the Markdown does not
   also link (worker skill, OpenAPI `SubmitRequest`); design 6 allows 20.
   The Files list therefore links every file by its `fileUrl`, which keeps
   all of them outside the cap. Design 6.4 asked for names and sizes; the
   links are added.
2. Live, the complete after a PUT to `uploadUrl` answers `409
   upload_not_pending` (capture of 2026-11-10), so design 6.1's check of
   the returned size and digest only runs when the complete answers 200.
   After a 409 the check rests on the exact byte count the PUT URL is
   signed for. Recorded in `docs/market.md`.
3. Deletes are not journaled: they are idempotent, best effort and not
   retried, and the market deletes an unused file 7 days after its upload.
   An upload whose answer was lost after the reserve may leave an unbound
   file only the market's sweep removes (the client cannot know its id).
4. The general `limits.files` (default 20) still applies and counts the
   Markdown and evidence, so with the default config at most 19 output
   files pass; the 21-file test widens `limits.files` so the delivery
   limit is what it checks. Not changed: it is the operator's config.
5. The secret scan reads every output file lossily as UTF-8, not only
   text files: a token is ASCII, so this also covers binary files.
6. Rework and redo generations upload their files again (a submit is a
   complete version; uploads are looked up within the generation).
7. Not test-first: `three_answerless_uploads_of_one_file_decline` was
   added after the implementation, from my own review, together with a fix
   (a `lost` result from an earlier round now counts toward the cap). The
   `lost` path itself (an intent left open by a crash mid-call) has no
   test; it needs a crash inside the fake's call.
8. No live run: no hire was taken, nothing was spent, and no file was
   uploaded to the live market in this section.

### Files changed in this section

`crates/nearbee-core/src/{market,serve}.rs`,
`crates/nearbee-core/tests/serve.rs`,
`crates/nearbee-market/src/{lib,wire}.rs`,
`crates/nearbee-market/tests/{client,contract}.rs`,
`crates/nearbee/src/replay/market.rs`, `docs/market.md`,
`docs/architecture.md`, `docs/privacy.md`,
`openspec/changes/sandbox-toolchains/tasks.md`, this report.
`cargo test --workspace`: 856 passed, 0 failed, 5 ignored.

### Processes and temporary data

No process left running. `~/.cache/nearbee-sandbox-toolchains/` (gate
logs) is removed at the end; `~/.cache/nearbee-sandbox-toolchains-target`
is kept for the integrator's reruns.

### Gate tails (verbatim)

```
$ cargo fmt --all --check
fmt exit=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Checking nearbee-cloud v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee-cloud)
    Checking nearbee v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.76s
clippy exit=0

$ cargo test --workspace
   Doc-tests nearbee_market_fixture

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

test exit=0

$ openspec validate sandbox-toolchains --strict
Change 'sandbox-toolchains' is valid
validate exit=0
```

## Review fixes (REVIEW.md findings 1 to 7, 2026-11-13)

1. **429 and other retryable 4xx on an upload write.** `upload_failed` in
   `serve.rs` treats `408`, `425` and `429` (on the reserve or the
   complete) like an unknown outcome: an `unknown` result with the code,
   `Uploads::Pending`, one of the file's `UPLOAD_ATTEMPTS`. Any other
   refusal still declines. Tests: `a_rate_limited_upload_is_retried_and_delivered`
   (429 and 408 on the reserve, 429 and 425 on the complete),
   `three_rate_limited_uploads_of_one_file_decline`,
   `a_refused_upload_declines_and_deletes_the_earlier_uploads`.
2. **Size and digest check on the 409 path.** The market API has no read
   of a stored file's `byteSize` or `sha256`: the OpenAPI's file
   operations are `POST /v1/files`, `POST /v1/files/{id}/complete`,
   `DELETE /v1/files/{id}`, `GET /v1/files/{id}/raw` and
   `GET /files/{token}/{filename}` (downloads only), and `docs/market.md`
   records none. So no check was added; design 6.1, the `hire-serving`
   delta ("Manifest with output files") and `docs/market.md` now say what
   is checked: the complete's `byteSize` and `sha256` when it answers
   `200`; after `409 upload_not_pending` only the successful PUT of the
   exact bytes to the URL signed for the reserved size. Only that code
   counts as done (the client already matched the code, not the status).
3. **`rust-toolchain.toml` in the sandbox.** Reproduced on the current
   image (`--network none`, uid 1000, `channel = "1.88"`): "could not
   create temp file /usr/local/rustup/tmp/...: Permission denied".
   `sandbox/Dockerfile` sets `RUSTUP_TOOLCHAIN=${RUST_VERSION}` beside
   `GOTOOLCHAIN=local`; in the rebuilt image the same probe prints
   `cargo 1.100.0`. Recorded in `docs/apb.md`, design 2.3 and a new
   `apb-sandbox` requirement "Toolchains stay pinned".
4. **Deletes before the journal.** Every deletion serve makes goes through
   `delete_uploads`, which first appends a `deleted` result (with the file
   id) on the upload's operation, then calls `DELETE`;
   `completed_uploads` leaves out operations with a `deleted` result. The
   fake market's `delete_file` asserts the `deleted` result is on disk
   before the call. Test: `a_crash_after_deleting_uploads_never_submits_a_deleted_link`
   (A uploaded, B mismatch, the journal cut before the decline intent; the
   next round uploads A again and submits only new links).
5. **Unbound file after a crash mid-upload; missing `fileUrl`.** The
   `Market` seam is split: `reserve_file` and `complete_file` (with
   `upload_file` kept as a provided method that chains them). Serve
   journals a `market_upload` intent, closes it `reserved` with the file
   id and link, then opens a `market_upload_complete` intent naming the
   file id before the PUT. On the next round an open intent is closed
   `lost` (with the file id when known) and every reserved file id with no
   answered complete and no `deleted` result is deleted (journaled first).
   The client's `reserve_file` deletes the reservation when a linked
   reserve answers without `fileUrl`. Tests:
   `a_crash_mid_upload_deletes_the_reserved_file` (serve),
   `a_linked_reserve_without_a_file_url_is_refused_before_the_put` (client,
   now expects the `DELETE`). Still open, recorded in design 6.3: a lost
   reserve answer (the id never reached the journal) or a resent reserve
   leaves a slot only the market's 7-day sweep removes.
6. **pip-audit dependency tree.** `sandbox/pip-audit.lock` pins pip-audit
   2.10.1 and its 27 dependencies (pip included) by version and SHA-256,
   generated with `uv pip compile --generate-hashes` for Python 3.14 on
   x86_64 Linux (the versions equal those in the previous image). The
   Dockerfile installs it with `pip install --require-hashes --no-deps`.
7. **Yarn hosts.** `registry.yarnpkg.com` and `repo.yarnpkg.com` added to
   `sandbox/egress.allow`, design 1, the `apb-sandbox` delta and
   `docs/apb.md` item 7. The statement's host list changes again for
   every sandboxed listing (drift check holds them until republished).

### Test-first notes

The serve and client tests were written before the code. The serve tests
could not compile until the seam existed, so they were proven against the
fix by mutation instead: with the `deleted` filter, the orphan deletion and
the retryable mapping each switched off, all four new serve tests failed.
The client test failed on its assertion (one request, no `DELETE`) before
the change. The image changes have no automated test; they were checked by
a build and probes (below).

### Image check

`NEARBEE_BIN=$HOME/.local/share/nearbee/bin/nearbee sandbox/build.sh
reviewfix` built `nearbee-sandbox:reviewfix` (the live `local` tag and the
running instance were not touched; the `reviewfix` tag was removed after the
probes). Inside it, `--network none --user 1000:1000`: `pip-audit 2.10.1`,
`go1.27.4`, `cargo 1.100.0`, `uv 0.13.8`, `osv-scanner 2.7.1`, `cc 15.2.0`,
`RUSTUP_TOOLCHAIN=1.100.0`, 27 packages in the pip-audit venv, and the
`channel = "1.88"` probe printed `cargo 1.100.0`. The yarn hosts were not
probed through the egress proxy (that needs the instance restarted with the
new list; not done).

### Files changed in this section

`crates/nearbee-core/src/{market,serve}.rs`,
`crates/nearbee-core/tests/serve.rs`, `crates/nearbee-market/src/lib.rs`,
`crates/nearbee-market/tests/client.rs`,
`crates/nearbee/src/replay/market.rs`, `sandbox/Dockerfile`,
`sandbox/egress.allow`, `sandbox/pip-audit.lock` (new), `docs/apb.md`,
`docs/architecture.md`, `docs/market.md`,
`openspec/changes/sandbox-toolchains/{design.md,REPORT.md}`,
`openspec/changes/sandbox-toolchains/specs/{hire-serving,apb-sandbox}/spec.md`.
`cargo test --workspace`: 861 passed, 0 failed, 5 ignored.

### Gate tails (verbatim)

```
$ cargo fmt --all --check
fmt exit=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Checking nearbee-market v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee-market)
    Checking nearbee-cloud v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee-cloud)
    Checking nearbee v0.1.0 (/home/techmeat/projects/tools/nearbee/crates/nearbee)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.70s
clippy exit=0

$ cargo test --workspace

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

test exit=0

$ openspec validate sandbox-toolchains --strict
Change 'sandbox-toolchains' is valid
validate exit=0
```
