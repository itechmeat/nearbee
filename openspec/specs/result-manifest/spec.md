# result-manifest Specification

## Purpose
Defines the result manifest a market playbook writes to `out/result.json` and the checks Nearbee runs on it before anything reaches a buyer.

## Requirements

### Requirement: Manifest version 1
A manifest SHALL be a JSON object with `version: 1`, a required `markdown` entry, an optional `files` list and an optional `evidence` list. Every entry SHALL carry `path` (relative to `out/`), `size` (bytes) and `sha256` (lowercase hex); evidence entries SHALL also carry `kind` and `visibility` (`public` or `private`).

#### Scenario: Minimal valid manifest
- **WHEN** `out/result.json` names `deliverable.md` with its true size and hash and nothing else
- **THEN** validation succeeds with one Markdown entry, no files and no evidence

#### Scenario: Unknown version
- **WHEN** the manifest has `version: 2`
- **THEN** validation fails with an unsupported-version error

#### Scenario: Duplicate path
- **WHEN** two entries name the same path
- **THEN** validation fails naming the path

#### Scenario: Markdown in a subdirectory
- **WHEN** the `markdown` entry is `docs/deliverable.md` inside `out/`
- **THEN** validation accepts it, since any contained path of normal components is allowed for every entry

#### Scenario: Unknown field
- **WHEN** an entry carries a field the schema does not define
- **THEN** validation fails naming the field

### Requirement: Containment
Every path in the manifest SHALL resolve to a regular file inside the worktree's `out/` directory. Absolute paths, `..` components, symbolic links (on the entry or any parent, including `out/` itself), and non-regular files SHALL be rejected, also when a process changes the tree while validation runs: `out/` is opened once and every path is resolved from that descriptor without following links and without resolving a path by name twice.

#### Scenario: Traversal
- **WHEN** an entry's path is `../secrets.txt`
- **THEN** validation fails with a containment error and reads nothing outside `out/`

#### Scenario: Symlinked out directory
- **WHEN** `out` is a symbolic link to a directory outside the worktree
- **THEN** validation fails with a containment error

#### Scenario: Parent replaced by a link during validation
- **WHEN** after `out/` is opened, `out/reports` is moved outside `out/` and replaced by a symbolic link to it
- **THEN** reading `reports/d.md` fails with a containment error

#### Scenario: Symlinked entry
- **WHEN** `out/report.md` is a symbolic link, even to a file inside `out/`
- **THEN** validation fails with a containment error

### Requirement: Integrity
Validation SHALL read each file at most once with a bounded read, compare the actual byte count and SHA-256 with the manifest, and return the validated bytes so that what is later delivered is exactly what was checked.

#### Scenario: Hash mismatch
- **WHEN** a file's content does not match its `sha256`
- **THEN** validation fails naming the path

#### Scenario: File larger than the limit
- **WHEN** a file exceeds the configured per-file limit
- **THEN** validation fails without reading the file past the limit

### Requirement: Markdown limit
The Markdown entry SHALL be valid UTF-8 and at most the configured Markdown limit (default 262,144 bytes), and the final assembled deliverable text (Markdown plus any appended blocks) SHALL be checked against the same limit before delivery.

#### Scenario: Oversized Markdown
- **WHEN** the Markdown file is 262,145 bytes
- **THEN** validation fails with a size error

#### Scenario: Footer pushes it over
- **WHEN** a 262,000-byte Markdown entry gets a 500-byte appended block
- **THEN** the final-size check fails

### Requirement: Fail closed
A missing, unreadable or invalid manifest SHALL produce a validation error and never a partial result; no report, summary or node output SHALL stand in for it.

#### Scenario: No manifest
- **WHEN** a run succeeded but `out/result.json` does not exist
- **THEN** the result is a validation error stating the manifest is missing
