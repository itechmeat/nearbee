# Spec Delta

## MODIFIED Requirements

### Requirement: Review size cap
The pack SHALL keep the request at or below 256 KiB: it SHALL omit every file that does not fit as `size_cap` and keep packing the next ones. `audit/review-coverage.json` SHALL record the count of omitted files per reason and at most 1,000 omitted paths; the deliverable's coverage section SHALL list at most 50 omitted paths grouped by reason, each group with its count and "and N more" for the rest.

#### Scenario: Large repository
- **WHEN** the snapshot's text passes 256 KiB
- **THEN** every file that does not fit is omitted as `size_cap`, smaller later files are still packed, the request stays below the cap and the omitted files are listed

#### Scenario: Many omitted files
- **WHEN** more than 1,000 files are omitted
- **THEN** the coverage file keeps the counts per reason and 1,000 paths, and the deliverable lists 50 paths grouped by reason with "and N more"

#### Scenario: Binary file
- **WHEN** a listed file is not valid UTF-8
- **THEN** it is not packed and is listed as omitted

### Requirement: Review call
The review SHALL make one signed call through `nearbee signed-call`'s code with the request `stream: false`, `max_tokens: 16384`, `chat_template_kwargs: {"thinking": false}` and `response_format: {"type": "json_object"}`, a system message with the step's rules and a user message holding the brief, the feedback and the pack as labelled data blocks; its state SHALL live in `audit/signed/<execution_id>/`.

#### Scenario: Rework
- **WHEN** a rework runs the review with a new execution id
- **THEN** it makes a new signed call and does not reuse the earlier answer

#### Scenario: Reasoning off
- **WHEN** the review request is built
- **THEN** its JSON has exactly the keys `model`, `stream`, `max_tokens`, `chat_template_kwargs`, `response_format` and `messages`, `chat_template_kwargs` is exactly `{"thinking": false}` and `response_format` is exactly `{"type": "json_object"}`
