# Spec Delta

## MODIFIED Requirements

### Requirement: Delivery
On a succeeded run serve SHALL validate the manifest, recheck any `tee_signature` evidence, append the signature section, the evidence section (attested hires) and the statement, check the result against the Markdown limit and the secret scan, upload every output file the manifest lists through the market's file API within the file limits, snapshot it and submit the Markdown with the file references; a failed, aborted or timed-out run, an invalid manifest, a file past a limit or with a secret SHALL be declined `failed` with a short reason; a run report SHALL never be submitted.

#### Scenario: Invalid manifest
- **WHEN** the run succeeded but its manifest fails validation
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Footer pushes the deliverable over the limit
- **WHEN** the playbook's Markdown fits the limit but with the statement appended it does not
- **THEN** serve declines with `failed` and submits nothing

#### Scenario: Manifest with output files
- **WHEN** a succeeded run's manifest lists `flow.json` beside the Markdown
- **THEN** serve uploads `flow.json`, checks the size and digest the complete reports against the manifest (when the complete answers `409 upload_not_pending` and reports none, the check is the successful PUT of the exact bytes to the upload URL signed for the reserved size), and submits the Markdown with that file's reference

#### Scenario: Rate-limited upload
- **WHEN** the market answers an upload write with `429`
- **THEN** serve leaves the delivery pending, counts one of the file's three attempts, and uploads the file again on a later round instead of declining

#### Scenario: Crash after the reserve
- **WHEN** serve restarts after a file's reserve was journaled but before its complete was
- **THEN** it records the upload as lost, deletes the reserved file, and uploads the file again

#### Scenario: Crash after a deletion
- **WHEN** serve restarts after it deleted uploaded files and before it recorded the decline
- **THEN** no deleted file is reused: the journal records each deletion before the call, and a later submit carries only files uploaded again

#### Scenario: Resume after a crash
- **WHEN** serve restarts after a file upload completed but before the submit
- **THEN** it submits with the recorded file reference and does not upload the file again

#### Scenario: Too many files
- **WHEN** a manifest lists 21 files
- **THEN** serve declines `failed` naming the 20-file limit and uploads nothing

#### Scenario: Recovered submit
- **WHEN** serve restarts after the submission snapshot was written
- **THEN** it resends the snapshotted bytes, statement, evidence and file references included, without assembling them again
