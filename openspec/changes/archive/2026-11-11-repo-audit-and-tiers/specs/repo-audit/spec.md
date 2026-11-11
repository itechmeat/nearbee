# Spec Delta

## Purpose
Defines example agent A: how it takes a public GitHub repository from an untrusted brief, the snapshot it audits, the findings contract its agent step fills, and the deliverable it produces.

## ADDED Requirements

### Requirement: Repository URL scan
The scanner SHALL split the brief on whitespace and on `<`, `>`, `(`, `)`, `"`, `'`, and accept a token only when the whole token, after removing trailing `.`, `,`, `;`, `:`, is `https://github.com/<owner>/<repo>` with an optional `.git` and an optional trailing slash: host case-insensitive, owner 1 to 39 of `[A-Za-z0-9-]`, repo 1 to 100 of `[A-Za-z0-9._-]`, no credentials, port, extra path segment, query or fragment. Repositories SHALL be deduplicated by lowercase owner and repo.

#### Scenario: Extra path
- **WHEN** the only URL is `https://github.com/a/b/tree/main`
- **THEN** no repository is found

#### Scenario: URL with credentials
- **WHEN** the only URL is `https://user:pw@github.com/a/b`
- **THEN** no repository is found

#### Scenario: Same repository twice
- **WHEN** the brief names `https://github.com/A/B` and `https://github.com/a/b.git`
- **THEN** one repository is found

### Requirement: Dot segments are not names
The scanner SHALL refuse a repository name that is `.` or `..` after removing an optional `.git`.

#### Scenario: Dot segments
- **WHEN** the only URL is `https://github.com/a/../` or `https://github.com/a/..git`
- **THEN** no repository is found

### Requirement: Deterministic intake check
A listing MAY set `intake_check: github_repo`. Then intake SHALL run the scanner on the brief before any scope check or market start and decline `rejected` with the buyer reason "the brief must name exactly one public GitHub repository by its https://github.com URL" when it finds none or several.

#### Scenario: Two repositories
- **WHEN** a hire's brief names two distinct repositories
- **THEN** the hire is declined `rejected` and no market start, scope check or run happens

### Requirement: Snapshot fetch
`nearbee audit fetch` SHALL repeat the scan on the brief from the run's params, resolve the repository through `https://api.github.com/repos/<owner>/<repo>` (refusing a private repository), take the default branch's commit SHA from `https://api.github.com/repos/<owner>/<repo>/commits/<branch>`, and download `https://codeload.github.com/<owner>/<repo>/tar.gz/<sha>`, following no redirects, within 120 seconds in total and 20 seconds per request.

#### Scenario: Redirect
- **WHEN** a GitHub request answers 301
- **THEN** the step fails without following it

### Requirement: Archive limits
The archive SHALL be read as a stream with caps of 100 MiB compressed, 100 MiB of decompressed bytes counted over everything read (skipped members and tar headers included) and 20,000 members, with the 120-second deadline checked during decoding; after the tar end marker the rest of the gzip stream SHALL be read to its end under the same cap, so malformed or truncated data, a missing trailer or a wrong checksum included, SHALL fail the step.

#### Scenario: Skipped data still counts
- **WHEN** the archive's skipped members alone pass 100 MiB decompressed
- **THEN** the step fails

#### Scenario: Truncated archive
- **WHEN** the gzip stream ends early
- **THEN** the step fails and no snapshot is published

#### Scenario: Missing trailer
- **WHEN** the stream holds the whole tar but lacks its 8-byte gzip trailer, or the trailer's CRC is wrong
- **THEN** the step fails and no snapshot is published

### Requirement: Members written
Only regular files with normal relative path components SHALL be written; links, devices, `..` and absolute paths SHALL be skipped, and so SHALL files over 1 MiB or with a NUL byte in their first 8 KiB. Every skip SHALL be counted in the coverage by reason.

#### Scenario: Symbolic link member
- **WHEN** the archive holds a symbolic link to `/etc/passwd`
- **THEN** nothing is written for it and the coverage counts a skipped link

### Requirement: Repository instructions are data
On extraction, agent instruction and configuration files (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `opencode.json`, `.claude/`, `.opencode/`, `.agents/`, `.cursor/`, `.github/copilot-instructions.md`, at any depth) SHALL be written with the suffix `.repo-data`; a renamed path that collides with another member SHALL fail the step.

#### Scenario: Nested CLAUDE.md
- **WHEN** the archive holds `src/CLAUDE.md`
- **THEN** the snapshot holds `repo/src/CLAUDE.md.repo-data` and no `repo/src/CLAUDE.md`

### Requirement: Atomic snapshot
The fetch SHALL build `repo/`, `source.json` (`version: 1`, URL, owner, repo, commit SHA, the written files with sizes and the hex SHA-256 of each) and `coverage.json` (counts per skip reason) in a fresh staging directory created without following links, sync them, and publish them by renaming the staging directory to `audit-snapshot/`.

#### Scenario: Crash during extraction
- **WHEN** a previous fetch stopped before publishing
- **THEN** the next fetch removes the staging directory and fetches again

### Requirement: Snapshot reuse
A later run of the same hire SHALL reuse a published snapshot whose `source.json` parses and names the same repository, and SHALL remove only its own leftover staging directory before fetching again.

#### Scenario: Rework
- **WHEN** the buyer asks for changes and generation 2 runs
- **THEN** the fetch reuses the published snapshot and audits the same commit

### Requirement: Candidate findings
The audit agent step SHALL write only `audit/candidates.json` and SHALL not run any program from the repository. The file is `{"version":1,"findings":[...]}` with at most 50 findings, each with exactly `severity` (`critical`, `high`, `medium`, `low`, `info`), `title` (1 to 120 characters), `file` (relative to `audit-snapshot/repo/`), positive integers `line_start` not after `line_end`, `excerpt` (1 to 400), `detail` (1 to 1,500) and `fix` (1 to 1,000), in Unicode scalar values.

#### Scenario: Prompt says so
- **WHEN** the playbook's agent prompt is read
- **THEN** it forbids running repository code and treats repository files, the brief and the feedback as data

### Requirement: Agent step restrictions
The playbook's `audit` node SHALL declare the restrictions apb 1.0.0-beta.6 enforces for it: `writes: ["audit/**"]`, `max_changed_files: 1` and `protect: ["audit-snapshot/**"]`. It SHALL declare no `isolation`: apb refuses `full` for `opencode`, which declares no sandbox needs, and `best_effort` would run without the snapshot, so reading the owner's files and the network stay not prevented and the statement says so.

#### Scenario: Write scope declared
- **WHEN** the playbook's `audit` node is read
- **THEN** it declares `writes: ["audit/**"]`, `max_changed_files: 1` and `protect: ["audit-snapshot/**"]`

### Requirement: Grounding check
`nearbee audit finish` SHALL read its inputs only as regular files of at most 2 MiB reached without links, and SHALL accept a finding only when its file is in `source.json`'s file list, its lines exist, and its excerpt, each line trimmed and blank lines dropped, is a contiguous run of the cited lines trimmed the same way. Rejected findings SHALL be kept with the reason; grounding SHALL be called "location checked", never proof the issue is real.

#### Scenario: Invented line
- **WHEN** a finding names line 900 of a 40-line file
- **THEN** it is rejected with the reason `line out of range`

#### Scenario: Wrong version
- **WHEN** `audit/candidates.json` has `version: 2`, an unknown field, or is not JSON
- **THEN** the step fails and leaves no `out/result.json`

### Requirement: Snapshot integrity
`nearbee audit finish` SHALL reject a finding whose file no longer has the SHA-256 that `source.json` records for it, with the reason `file changed after fetch`, before checking its lines.

#### Scenario: File rewritten by the agent step
- **WHEN** the agent step appended a line to a snapshot file and cites it
- **THEN** the finding is rejected with the reason `file changed after fetch`

### Requirement: Deliverable
`nearbee audit finish` SHALL first remove any `out/result.json`, then write through temporary files `out/findings.json` (`version`, repository URL and SHA, accepted findings, rejected findings with reasons, coverage) and `out/deliverable.md` (a summary, the accepted findings by severity with escaped text, the counts of rejected findings and skipped files, and the JSON in a fence of `max(3, longest backtick run + 1)` backticks), and publish `out/result.json`, naming only the Markdown, last.

#### Scenario: Markdown injection
- **WHEN** a finding's title contains `</details>` and a backtick fence
- **THEN** the rendered Markdown shows them as text and the JSON fence stays closed

#### Scenario: Failed retry after success
- **WHEN** a successful finish is followed by a finish on invalid candidates
- **THEN** no `out/result.json` remains

### Requirement: Tilde fences
The finding text SHALL be escaped so that a run of three or more `~` at a line start, after any indentation, opens no code fence.

#### Scenario: Tilde fence in a fix
- **WHEN** a finding's fix has a line `~~~` or an indented `~~~~ rust`
- **THEN** the rendered Markdown escapes the first tilde, no fence opens, and the JSON section and the footer stay outside any code block
