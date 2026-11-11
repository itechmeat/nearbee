# Tasks: doc digest and paid demo

Implementer rules: `docs/process.md`. Change `doc-digest-and-paid-demo`,
worktree `~/projects/tools/nearbee-wt-s5`,
`CARGO_TARGET_DIR=$HOME/.cache/nearbee-doc-digest-and-paid-demo-target`,
report `.reviews/doc-digest-and-paid-demo-implementer.md` (when it exists,
read its "Next session handover" first and append). Before a group, read
its decision in `design.md` and the spec it names. NEAR AI Cloud is tested
against local `wiremock` servers whose base URL the code takes as a
parameter, and against recorded fixtures; no billed call in this change's
tests. Every hire is the owner's own test (D-023). No spend, no market
write. Tasks marked **integrator** or **owner** are not yours.

## 1. Cloud client (design 1; spec `signed-call`)

- [x] 1.1 Fixtures first: the signature answer copied verbatim from `.agents/skills/near-ai-cloud/references/chat-verification.md` (the "Response" example plus `signature_kind`), and the classify and chat completion answers from the docs.near.ai pages, each with its source and read date in the fixture's `note`. Mocks serve these fixtures, never hand-written bodies. Then tests of `chat_exact`, `signature`, the report with `provider`, `classify`, `chat_json` in `crates/nearbee-cloud` against `wiremock`: headers, no redirects, no proxy, body caps, a non-2xx returned with its status, `NotYet` for 404 and 2xx `error_code`, classify drops span text, `finish_reason` returned, unknown fields ignored, a missing required field fails naming it, the key never in errors. They fail before the code exists.
- [x] 1.2 Implement the client; `cargo test -p nearbee-cloud` passes.

## 2. signed-call (design 2; spec `signed-call`)

- [x] 2.1 Tests for every scenario of spec `signed-call` against `wiremock`: streaming request, no `max_tokens`, `max_tokens` 16,385, request over 1 MiB, response over 4 MiB, model not allowed, compressed answer, absent `Content-Encoding` accepted, the request bytes sent equal the file bytes exactly, empty `model_attestations`, gateway signature, signer not in the preflight report (no further report fetched), addresses compared case-insensitively, completion id with a slash, signature after two 404s, resume from `responded`, resume from `sending` with a stored response fetches only the signature, fail from `sending` without one, 402 answer then retry (one request in total), `finish_reason: length` refused, deadline reached during the signature retries leaves `responded`, `signed` makes no call, evidence files written with the exact `signature.json` fields.
- [x] 2.2 Implement `signed_call` and the `nearbee signed-call` subcommand; `cargo test -p nearbee` passes for the new tests.

## 3. Direct models through listing, tiers, statement, evidence, run params (design 3, 5; specs `listing-publish`, `privacy-tiers`, `data-statement`, `run-evidence`)

- [x] 3.1 Tests: listing `direct_models` and `signed_models` parsing (order kept, a `nearai/` prefix or a duplicate refused naming it, a signed model outside the direct list refused); attested refuses a non-`nearai` direct model and plain refuses one not in the catalog; `NEARAI_CLOUD_URL`, a `NEARBEE_CLOUD_` variable or `https_proxy` in the instance environment makes routes unknown; the statement names direct models and the signed step; the fingerprint of a listing without direct models is unchanged from S4 (fixture); reordering direct models changes the fingerprint; the drift pass of a listing with direct models whose apb facts are unchanged reports Clean (no drift); evidence models include direct models; a caller param named `direct_models` is refused; `sent_params` carries the snapshot's list; a generation started before a republish keeps its frozen list.
- [x] 3.2 Implement, including `ExecutionFacts::with_direct_models` at `nearbee list` and in `statement::drift`, the snapshot fields, `StartRequest.direct_models` and the `apb-instance.sh` refusal of the redirecting variables; check that `openai/privacy-filter` is in `docs/reference/near-ai-cloud-models.json` with owner `nearai` (add it from the live public `GET /v1/models` with the fetch date if missing). Fetch one attestation report each for `Qwen/Qwen3-VL-30B-A3B-Instruct` and `openai/privacy-filter` (not billed; key only). Record status, size and `model_attestations` count in `docs/near-ai-cloud.md`. If either has no model attestation, stop and report to the integrator before task 4. Targeted tests pass.

## 4. Agent B commands (design 4; spec `doc-digest`)

- [x] 4.1 Tests for `pages` with generated PDF and image fixtures (skip with a clear message when `pdftoppm` or `magick` is missing): sniffing, 2 MP cap, long side at most 2000 px, 40-page cap with skipped count, 11 files refused, no usable file, total size cap, a path outside `inputs/` and a symlink refused, an encrypted PDF skipped, state under `digest/<execution_id>/`, a missing or malformed execution id refused.
- [x] 4.2 Tests for `extract` and `classify` against `wiremock`: request shape with the image part, resume, one retry then fail, `truncated` stored when `finish_reason` is not `stop`, PII counts without span text, model outside `direct_models` refused.
- [x] 4.3 Tests for `compose`: request shape and caps (at most 900 KiB, pages cut last first and counted), feedback included as a labelled data block capped at 4,000 characters, signed call through the mock, schema refusal, a model answer with `pii` refused, fenced JSON answer accepted, figure on the wrong page dropped, a value `1` dropped, escaping (links, images, fences, `https://x.y`, `www.x.y`, `a@b.c`), page count, skipped and truncated pages and PII categories without values in the deliverable, `digest.json` with `pii` from `pii.json`, `usage.json` lists each call's usage, `out/result.json` last with the four evidence entries and their kinds, validates with `nearbee_core::manifest::validate`; a second execution id makes a new signed call and re-extracts; the same execution id resumes.
- [x] 4.4 Implement `nearbee digest pages|extract|classify|compose`; the tests of 4.1 to 4.3 pass.

## 5. Serve delivery (design 6; spec `hire-serving`)

- [x] 5.1 Tests in `crates/nearbee-core/tests/serve.rs` and unit tests of `signature::recheck`: a valid `tee_signature` entry renders the section (fields in code spans, the "what you can verify" wording) and a receipt link whose base64url JSON decodes to the seven fields in order; an edited `response.bin` declines `failed` with `signature_mismatch`; a gateway kind or a model outside the frozen direct models declines; a Markdown link in `completion_id` declines; a generation with `signed_models` and a run without the entry declines `signature_missing`; the private bytes never appear in the submission; the private files are snapshotted only when the submit intent opens; the limit and secret scan see the section; a restart resends the snapshot.
- [x] 5.2 Implement `nearbee-core/src/signature.rs` and the delivery changes; the tests of 5.1 and the existing serve tests pass.

## 6. Playbook, listing, docs (design 7, 8)

- [x] 6.1 `playbooks/doc-digest/` playbook, scripts and `listing.doc-digest-private.yaml` (`direct_models`, `signed_models`, `file_types: [pdf, png, jpg, jpeg]`, the `direct_models` param, `compose` with `expected_duration: 6m`); a test loads and materialises it, the attested check passes on fixture facts, and a `.jpg` attachment passes the listing's file types.
- [x] 6.2 Docs: `docs/decisions.md` D-026 (vision, classify and digest are direct calls from trusted script nodes; only the digest is signed; widens D-008); `docs/privacy.md` (direct models, the signed path as built, the receipt link, what the buyer can and cannot verify, cost not in daily loss); `docs/showcase-agents.md` agent B as built (no DOCX, 40-page cut, entities without personal data, the default profile note when it stays); `docs/near-ai-cloud.md` (signature and classify facts); `docs/architecture.md` (the per-execution digest directory); `playbooks/README.md`; `docs/stages.md` S5 status.
- [x] 6.3 Full native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `openspec validate doc-digest-and-paid-demo --strict`.

## 7. Live (integrator and owner)

- [ ] 7.1 **owner** Cloud credits (O-3), the `doc-digest-private` draft agent and token (O-2), the demo caller account with USDC (O-5), go-ahead for the spend and listings (O-6).
- [ ] 7.2 **integrator** Record live classify, completion and signature answers as replay fixtures (redacted) and rerun the contract tests; `nearbee list` of `doc-digest-private`; a live agent B hire with a scanned invoice; the receipt link verifies in the Verification Center; recorded in `docs/demo-log.md`.
- [ ] 7.3 **integrator** A live `repo-audit-fast` run on Cloud, then the paid A2A hire with `examples/x402-hire`; task id, settlement and payout in `docs/demo-log.md`.

## Workflow follow-up

- One review round (owner 2026-11-11), archive, pull request.
