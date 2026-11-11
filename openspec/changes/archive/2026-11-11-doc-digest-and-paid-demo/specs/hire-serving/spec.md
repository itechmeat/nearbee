# Spec Delta

## MODIFIED Requirements

### Requirement: Delivery
On a succeeded run serve SHALL validate the manifest, recheck any `tee_signature` evidence, append the response signature section, the generation's evidence section (attested hires) and data statement, check the assembled text against the Markdown limit and the secret scan, store it in the generation's snapshot and submit it; on a failed, aborted or timed-out run, an invalid manifest, or a manifest that lists output files, it SHALL decline with `failed` and a short reason; a run report or summary SHALL never be submitted.

#### Scenario: Invalid manifest
- **WHEN** the run succeeded but its manifest fails validation
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Manifest with output files
- **WHEN** the run's manifest lists files besides the Markdown
- **THEN** serve declines with `failed`, submits nothing, and tells the operator that output files are not supported yet

#### Scenario: Footer pushes the deliverable over the limit
- **WHEN** the playbook's Markdown fits the limit but with the statement appended it does not
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Recovered submit
- **WHEN** serve restarts after the submission snapshot was written
- **THEN** it resends the snapshotted bytes, statement and evidence included, without assembling them again

## ADDED Requirements

### Requirement: Response signature recheck
For a `tee_signature` entry serve SHALL pair it with exactly one `tee_request` and one `tee_response` entry in its directory, recompute both SHA-256 from the validated bytes, rebuild `<model>:<request hash>:<response hash>`, compare it with the entry, require `signature_kind` `provider_tee` and a frozen direct model, and snapshot the two private files; any mismatch SHALL decline `failed` with attention `signature_mismatch`.

#### Scenario: Response file edited
- **WHEN** `response.bin` changed after the signature was fetched
- **THEN** serve declines `failed` with `signature_mismatch` and submits nothing

#### Scenario: Gateway signature
- **WHEN** the entry's `signature_kind` is `gateway`
- **THEN** serve declines `failed` with `signature_mismatch`

### Requirement: Signature field formats
Before rendering, serve SHALL require `completion_id` to match `[A-Za-z0-9_-]{1,128}`, `signing_address` `0x` plus 40 hex digits, `signature` `0x` plus 130 hex digits, both hashes 64 lowercase hex digits, `signing_algo` `ecdsa`, and `model` in the frozen direct models; anything else SHALL decline `failed` with attention `signature_mismatch`.

#### Scenario: Link in the completion id
- **WHEN** `completion_id` holds a Markdown link
- **THEN** serve declines `failed` with `signature_mismatch`

### Requirement: Required signature
For a generation whose frozen snapshot has `signed_models`, serve SHALL require one valid `tee_signature` entry whose model is in that list, and SHALL decline `failed` with attention `signature_missing` when there is none. A statement snapshot that cannot be read or parsed SHALL never read as no direct or signed models: at the run start and at delivery it SHALL decline `failed` with attention `statement_missing`; only a legacy generation without a snapshot reads as none.

#### Scenario: Unreadable snapshot at delivery
- **WHEN** a statement generation's snapshot does not parse when its run has ended
- **THEN** serve declines `failed` with `statement_missing`, submits nothing, and never skips the signature requirement

#### Scenario: Signed listing without the entry
- **WHEN** a `doc-digest-private` run succeeds with a manifest that has no `tee_signature` entry
- **THEN** serve declines `failed` with `signature_missing` and submits nothing

### Requirement: Response signature section
The deliverable SHALL carry a section `Response signature`, between the playbook Markdown and the evidence section, with the completion id, model, signing address, both hashes, the signed text and the signature each in a code span, and a link `https://cloud.near.ai/verify?model=<URL-encoded id>#receipt=<receipt>`; the private bytes SHALL never be delivered.

#### Scenario: Receipt link
- **WHEN** an agent B deliverable is assembled
- **THEN** its receipt is compact JSON with the keys `completionId`, `model`, `signatureKind`, `signingAlgo`, `signingAddress`, `signedText` and `signature` in that order and the entry's values, encoded base64url without padding

### Requirement: What the buyer can verify
The section SHALL say that the signature binds a response with the shown hash produced in a model TEE, and that the buyer cannot recompute the hashes because the request and response bytes stay with the owner.

#### Scenario: Wording
- **WHEN** an agent B deliverable is assembled
- **THEN** the section states that the request and response bytes stay with the owner and the hashes cannot be recomputed by the buyer
