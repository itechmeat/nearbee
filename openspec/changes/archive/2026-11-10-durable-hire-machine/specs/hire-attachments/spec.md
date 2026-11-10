# Spec Delta

## Purpose

Brings the files a buyer attached to a job or to a rework message into the run safely: authenticated download from the market only, size and hash checks, safe names inside the hire worktree, and one `inputs` param for the playbook.

## ADDED Requirements

### Requirement: Origin and authentication
Serve SHALL download an attachment only from the configured market origin (same scheme, host and port after URL parsing), with the agent token as a bearer header, without following redirects, and SHALL refuse a `downloadUrl` with userinfo, a fragment or another origin; a relative `downloadUrl` resolves against the market origin.

#### Scenario: Foreign host
- **WHEN** an attachment's `downloadUrl` is `https://evil.example/x`
- **THEN** nothing is requested, the token is not sent, and the generation is declined `failed` with a generic reason

#### Scenario: Relative path
- **WHEN** `downloadUrl` is `/v1/jobs/<job>/attachments/<id>/raw`
- **THEN** serve requests it on the market origin with the bearer header

### Requirement: Size and hash
Serve SHALL refuse an attachment whose declared `byteSize` is negative or above the config's `limits.file_bytes`, SHALL stop reading once the streamed bytes exceed the declared size, and SHALL keep the file only when its length equals `byteSize` and its SHA-256 equals `sha256`.

#### Scenario: Hash mismatch
- **WHEN** the downloaded bytes do not hash to the declared `sha256`
- **THEN** the file is removed, the run does not start, and the generation is declined `failed`

#### Scenario: Stream longer than declared
- **WHEN** the server sends more bytes than `byteSize`
- **THEN** serve stops reading at `byteSize + 1` bytes and treats the file as a mismatch

### Requirement: Listing limits per generation
Before any download, serve SHALL check a generation's new files against the listing's `limits`: each new file's extension SHALL be in `file_types`, and the hire's stored files plus the new ones SHALL stay within `max_attachments` and `max_attachment_bytes` (declared sizes, summed); a breach SHALL decline the generation `failed` with a generic reason and download nothing.

#### Scenario: Rework attaches an excluded type
- **WHEN** the listing sets `file_types: [md]` and a rework message attaches `tool.exe`
- **THEN** nothing is downloaded, no run starts, and the generation is declined `failed`

#### Scenario: Rework goes over the hire's total
- **WHEN** the listing sets `max_attachments: 1`, the job stored one file and a rework message attaches another
- **THEN** nothing is downloaded and the generation is declined `failed`

### Requirement: Retry rules
A download answering 503 SHALL be retried in a later round after its `Retry-After` (default 30 seconds) until the generation's deadline; 401 stops the agent as for any call; 403 and 404 SHALL decline the generation `failed`.

#### Scenario: Still being scanned
- **WHEN** a download answers 503 with `Retry-After: 20`
- **THEN** serve records the due time, does not sleep in the round, and downloads again no sooner than 20 seconds later

#### Scenario: Quarantined
- **WHEN** a download answers 403
- **THEN** the generation is declined `failed` with a generic reason

### Requirement: Layout and names
Each verified file SHALL be stored as `inputs/<nn>-<safe name>` inside the hire's worktree, where `nn` is its index in the hire, at least two digits, given once in the order the files are first stored (the job's attachments, then each rework message's) and never reused, and the safe name keeps `[A-Za-z0-9._-]`, replaces other characters with `_`, drops leading dots and is at most 100 bytes.

#### Scenario: Unsafe name
- **WHEN** the attachment's filename is `../../.ssh/id_rsa`
- **THEN** it is stored as `inputs/00-_.._.ssh_id_rsa` (slashes replaced, the leading dots dropped), inside the worktree

#### Scenario: Same name in two reworks
- **WHEN** two rework messages each attach a different `a.pdf`
- **THEN** they are stored under two different indices and the first file is unchanged

### Requirement: Atomic and once
A file SHALL be written to a temporary name and renamed only after verification, and an attachment id already stored for the hire SHALL not be downloaded again.

#### Scenario: Already stored
- **WHEN** a rework row still lists the job's attachments that generation 1 stored
- **THEN** none of them is downloaded again

#### Scenario: inputs left unsynced by a crash
- **WHEN** serve stopped after creating `inputs/` and before syncing the worktree, and a download runs after the restart
- **THEN** the worktree and its parent are synced before any file in `inputs/` is recorded

#### Scenario: Crash during a download
- **WHEN** serve dies while a file is half written
- **THEN** after a restart no partial file is in `inputs/` and the download runs again

### Requirement: The inputs param
Serve SHALL pass the stored files to the run as the param `inputs`: a JSON array of paths relative to the worktree, in index order, `[]` when there are none; the buyer can never set `inputs`, `brief`, `feedback`, `hire_id` or `execution_id`.

#### Scenario: Two files
- **WHEN** a job has two verified attachments
- **THEN** the run's `inputs` param is `["inputs/00-a.pdf","inputs/01-b.csv"]`
