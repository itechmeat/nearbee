# Spec Delta

## MODIFIED Requirements

### Requirement: Candidate findings
The candidate step (the `audit` agent of `playbook.yaml`, the `review` script of `playbook.fast.yaml`) SHALL write only `audit/candidates.json` (the review also `audit/signed/` and `audit/review-coverage.json`) and run nothing from the repository.

#### Scenario: Prompt says so
- **WHEN** the playbook's agent prompt or the review step's system message is read
- **THEN** it forbids running repository code and treats repository files, the brief and the feedback as data

## ADDED Requirements

### Requirement: Candidates file shape
`audit/candidates.json` SHALL be `{"version":1,"findings":[...]}` with at most 50 findings (40 from the review step), each with exactly `severity` (`critical`, `high`, `medium`, `low`, `info`), `title` (1 to 120), `file` (relative to `audit-snapshot/repo/`), `line_start` not after `line_end` (positive), `excerpt` (1 to 400), `detail` (1 to 1,500) and `fix` (1 to 1,000), lengths in Unicode scalar values.

#### Scenario: Unknown field
- **WHEN** a finding carries a field other than the listed ones
- **THEN** the candidates file is invalid


### Requirement: Fast variant playbook
The fast listing SHALL use `playbooks/repo-audit/playbook.fast.yaml`: the nodes `fetch`, `review` and `finish` are script nodes, no node runs an agent and the playbook names no default profile; its listing SHALL name no profile and DeepSeek V4.1 Flash as its one direct and signed model. The premium listing SHALL keep `playbook.yaml`.

#### Scenario: No agent node
- **WHEN** `playbook.fast.yaml` is read
- **THEN** it has no `agent_task` node

### Requirement: Review packing
`nearbee audit review` SHALL pack the snapshot deterministically: the listed files that coverage does not mark skipped and that are valid UTF-8, in a fixed priority order (manifests and build files, source, config, tests, docs, other files; lock files, minified files `*.min.*`, source maps `*.map` and files under `dist/`, `build/`, `vendor/` or `node_modules/` last; ties by path), each under a header `=== FILE <path> (<n> lines) ===` with every line prefixed by its number and `| `.

#### Scenario: Same snapshot, same pack
- **WHEN** the review packs the same snapshot twice
- **THEN** the request bytes are identical

#### Scenario: Generated files last
- **WHEN** the snapshot holds a lock file, a minified file, a source map and files under `dist/`, `build/`, `vendor/` and `node_modules/`
- **THEN** they are packed after every other file

### Requirement: Data blocks cannot close themselves
The path in a pack header SHALL be JSON-quoted (control characters escaped), and the lines of the brief and of the change request SHALL be numbered like the pack, so no file name, brief or change request can add a marker line to the request.

#### Scenario: Path with a newline
- **WHEN** a file name holds a newline
- **THEN** its header carries the path JSON-quoted on one line and no other header line is created

#### Scenario: Brief closing its block
- **WHEN** the brief holds the line that ends the brief's data block
- **THEN** that line is numbered in the request and the block's real end marker appears once

### Requirement: Review size cap
The pack SHALL keep the request at or below 768 KiB: it SHALL omit every file that does not fit as `size_cap` and keep packing the next ones. `audit/review-coverage.json` SHALL record the count of omitted files per reason and at most 1,000 omitted paths; the deliverable's coverage section SHALL list at most 50 omitted paths grouped by reason, each group with its count and "and N more" for the rest.

#### Scenario: Large repository
- **WHEN** the snapshot's text passes 768 KiB
- **THEN** every file that does not fit is omitted as `size_cap`, smaller later files are still packed, the request stays below the cap and the omitted files are listed

#### Scenario: Many omitted files
- **WHEN** more than 1,000 files are omitted
- **THEN** the coverage file keeps the counts per reason and 1,000 paths, and the deliverable lists 50 paths grouped by reason with "and N more"

#### Scenario: Binary file
- **WHEN** a listed file is not valid UTF-8
- **THEN** it is not packed and is listed as omitted

### Requirement: Review call
The review SHALL make one signed call through `nearbee signed-call`'s code with the request `stream: false` and `max_tokens: 16384`, a system message with the step's rules and a user message holding the brief, the feedback and the pack as labelled data blocks; its state SHALL live in `audit/signed/<execution_id>/`.

#### Scenario: Rework
- **WHEN** a rework runs the review with a new execution id
- **THEN** it makes a new signed call and does not reuse the earlier answer

### Requirement: Unparsable review answer
An answer that is not a candidates object, or has more than 40 findings, SHALL fail the step with `review_unparsed` and the reason in `audit/review-coverage.json`, so the hire is declined `failed`.

#### Scenario: Unparsable answer
- **WHEN** the model answers text that is not the candidates JSON
- **THEN** the step fails, the coverage file records `review_unparsed`, and no candidates file is written

### Requirement: Nothing worth reviewing
When no source-like file (a manifest, source, config or test file) was packed, the review SHALL fail the step before any request with `nothing_packed` and the reason in `audit/review-coverage.json`, so the hire is declined `failed`.

#### Scenario: Only docs and lock files
- **WHEN** the packed files are only docs and lock files, or the only source file does not fit
- **THEN** no request is sent, the step fails and the coverage file records `nothing_packed`

### Requirement: Signed review evidence
`nearbee audit finish` SHALL, for a run with a non-empty `direct_models` run param (the fast playbook; premium never) whose `audit/signed/<execution_id>/signature.json` exists, copy the signature, request, response and model report into `out/evidence/` and list them as `tee_signature` (public), `tee_request`, `tee_response` and `tee_model_report` (private); otherwise the manifest has no evidence.

#### Scenario: Fast deliverable
- **WHEN** a fast audit finishes after a signed review
- **THEN** the manifest lists the four evidence entries and serve's signature recheck accepts it

#### Scenario: Premium deliverable
- **WHEN** a premium audit finishes with no signed call
- **THEN** the manifest has no evidence entries

#### Scenario: Planted signature on a premium run
- **WHEN** a premium run's worktree holds `audit/signed/<execution_id>/signature.json`
- **THEN** the manifest has no evidence entries
