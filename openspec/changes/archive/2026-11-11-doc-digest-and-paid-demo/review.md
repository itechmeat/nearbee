# Independent review

## Reviewer

A fresh Claude Opus context that did not write the artifacts, read-only on
the change folder, the repository and the code on `main` at S4; one round,
as the owner asked for one review round only. The author (Claude) checked each finding against
the code, applied the fixes below and decided every finding; there was no
re-review.

## Findings

1. design 2, 4, spec `doc-digest`, `signed-call`: a rework replayed the stored signed answer and reused extraction in the shared worktree; `feedback` was unused. Fixed: every digest command works in `digest/<execution_id>/`; feedback goes into the digest request as a labelled data block; requirement Execution directory and scenario Rework; tests in 4.3.
2. design 5, spec `data-statement`: the drift pass's facts lacked direct models, so every agent B hire would drift. Fixed: `ExecutionFacts.direct_models` filled by the caller through `with_direct_models` at `nearbee list` and in `statement::drift`; scenario No drift from direct models; test in 3.1.
3. design 3, 7: the reserved `direct_models` param did not fit the runner. Fixed: `StartRequest.direct_models`, `RESERVED_PARAMS` extended, `sent_params` adds the snapshot's frozen list, the playbook declares the param; tests in 3.1.
4. spec `signed-call`, `hire-serving`, design 6: the evidence file contract was incomplete. Fixed: exact `signature.json` fields, kinds `tee_signature`, `tee_request`, `tee_response`, `tee_model_report`, pairing by kind in one directory from the validated bytes.
5. design 2, spec `signed-call`: `sending` contradicted the spec and the response write was not atomic. Fixed: request and response written by temp file, sync, rename; `sending` with a stored response resumes; test in 2.1.
6. design 1, 2, spec `signed-call`: the time budget cut off a paid completion. Fixed: completion timeout 240 s, 360 s per invocation with a fresh bound on resume, signature retries `min(60 s, time left)`, `compose` `expected_duration: 6m`; test in 2.1.
7. design 2, spec `signed-call`: a failed completion answer left the step stuck. Fixed: terminal phase `refused` with the status, no resend, idempotency stated; scenario Refused completion; test in 2.1.
8. design 5, spec `data-statement`, `hire-serving`, `listing-publish`: the signed-step claim had no detectable source and was not enforced. Fixed: the `signed_step` fact dropped; listing key `signed_models` (subset of `direct_models`, fingerprinted, frozen); serve declines `signature_missing` without the entry; test in 5.1.
9. design 6: delivery code placement and the attention code were unspecified. Fixed: pure `signature::recheck` in `nearbee-core/src/signature.rs`, called at both validation points, typed error with an attention code, private snapshots only in the submit branch.
10. spec `hire-serving`, `signed-call`: untrusted `signature.json` fields went into Markdown and a URL. Fixed: requirement Signature field formats, code spans, the completion id checked before the signature URL; tests in 2.1 and 5.1.
11. design 4, spec `doc-digest`: buyer files were not contained and PDF rendering was not bounded. Fixed: paths under `inputs/` through `read_beneath`, copies under the execution directory, `pdftoppm -scale-to 2000`, `RLIMIT_AS`, `RLIMIT_CPU`, capped stderr, encrypted PDFs skipped; tests in 4.1.
12. design 7: `file_types` refused `.jpg`. Fixed: `[pdf, png, jpg, jpeg]`; test in 6.1.
13. tasks 1, 7: the new Cloud wire types had no recorded responses. Fixed: task 1.1 starts with documented fixtures with source and date; task 7.2 records live answers and reruns the contract tests.
14. tasks 3, design 5: evidence reports for the vision model and the privacy filter were unverified. Fixed: task 3.2 fetches one report each (not billed) and stops before task 4 when either lacks a model attestation; design 5.4 states the refusal.
15. spec `doc-digest`: the `digest.json` schema could not be validated. Fixed: exact model schema with bounds, `pii` from `pii.json` only, a `pii` key refused, figure matching normalised with a 2-character minimum; requirement Digest file; tests in 4.3.
16. proposal, spec `hire-serving`: what the buyer can verify was not said. Fixed with option (b): the section and `docs/privacy.md` say the signature binds a response hash produced in a model TEE and the bytes stay with the owner. Option (a) rejected: the response body may carry reasoning text quoting the documents and can exceed the Markdown limit.
17. spec `listing-publish`: canonical form and duplicates of direct models. Fixed: bare catalog ids, a prefixed or duplicated entry refused naming it.
18. spec `privacy-tiers`, design 1, 5: the redirecting-variable rule was vague. Fixed: named variables (`NEARAI_CLOUD_URL`, `NEARBEE_CLOUD_`, the proxy variables, any case), rule in `inspect.rs` and `apb-instance.sh`, commands built with `.no_proxy()` on the production base; serve's attestor keeps its override.
19. design 2, spec `signed-call`: the signer may be missing from the preflight report. Case-insensitive comparison fixed; the second report rejected: the response-signatures page (docs.near.ai, read 2026-11-11) forbids a later report as a substitute for preflight evidence and says instances of one model share one signing key; tests in 2.1.
20. design 4: the compose request could exceed the 1 MiB cap. Fixed: pages cut last first until the request is at most 900 KiB.
21. design 2, 4: truncated answers were not detected. Fixed: `finish_reason` other than `stop` refuses the signed call and marks an extracted page `truncated`; tests in 2.1 and 4.2.
22. design 4: the bare-URL rule was undefined. Fixed: `://`, `www.` and `@` escaped after `audit::escape`; test in 4.3.
23. design 7: the unused default profile entered the statement and the evidence. Fixed: omitted when apb accepts a playbook without it, otherwise kept and documented.
24. specs `hire-serving`, `listing-publish`, `run-evidence`: deltas dropped existing text. Fixed: dropped phrases restored word for word; run-evidence is a MODIFIED "Which models".
25. design 2: the on-disk state rule and the script-node key. Fixed: state location in the worktree with its reason and cleanup; the key from the apb unit, `NO_KEY` before any request.
26. design 1, 6: receipt encoding and an extra report client. Fixed: base64url without padding, compact JSON in the listed key order; the report reuses `CloudAttestor` with a `provider` option.
27. tasks: test gaps. Fixed: 1.1 split into tests and implementation, and every listed case added to 2.1, 3.1, 4.1 and 4.3; `usage.json` kept and tested.
28. spec `signed-call`: wording. Fixed: "a `Content-Encoding` header present and not `identity`" and "writes phase `refused` with reason `compressed`".

## Verdict

Ready for implementation: yes
