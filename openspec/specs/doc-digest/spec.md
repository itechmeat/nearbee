# doc-digest Specification

## Purpose
Defines example agent B: which buyer files it takes, how pages become images, how text is extracted and classified on TEE models, the digest contract, and the deliverable Nearbee renders from it.

## Requirements

### Requirement: Inputs
Agent B SHALL take the hire's attachments listed in the `inputs` param, each a relative path under `inputs/` read without following symlinks: PDF, PNG or JPEG, identified by their leading bytes, at most 10 files and 25 MiB in total. An encrypted or unreadable PDF SHALL be skipped and counted. A hire with no usable file SHALL fail the step with "no PDF, PNG or JPEG file was attached".

#### Scenario: Only a text file
- **WHEN** the only attachment is a `.txt` file
- **THEN** the step fails with the no-usable-file message

#### Scenario: Path outside inputs
- **WHEN** an `inputs` entry is `../secret.pdf` or a symlink
- **THEN** the entry is refused and never read

### Requirement: Execution directory
Every digest command SHALL keep its state in `digest/<execution_id>/` of the worktree, so a retried step of the same execution resumes and a rework or redo starts fresh without reading another execution's directory.

#### Scenario: Second execution
- **WHEN** a rework runs with a new execution id after a delivered digest
- **THEN** the pages are extracted again and a new signed call is made

### Requirement: Pages
`nearbee digest pages` SHALL render PDF pages with `pdftoppm` at most 2000 pixels on the long side and normalise images with ImageMagick using explicit `png:` or `jpeg:` input prefixes and resource limits, each page at most 2 megapixels, in input order, at most 40 pages; pages past 40 SHALL be skipped and counted. Every tool SHALL run with memory, CPU-time and wall-clock limits.

#### Scenario: Fifty pages
- **WHEN** the attachments hold 50 pages
- **THEN** 40 pages are rendered and the page list records 10 skipped

### Requirement: Extraction
`nearbee digest extract` SHALL make one non-streamed call per page to `Qwen/Qwen3-VL-30B-A3B-Instruct` with the page as an image content part and a fixed transcription prompt, store each page's text with a `truncated` flag in `pages/<n>.json`, and skip pages already stored. A page call that fails twice SHALL fail the step.

#### Scenario: Resume
- **WHEN** the step is retried after pages 1 to 7 were stored
- **THEN** only pages 8 onward are requested

### Requirement: Classification
`nearbee digest classify` SHALL send each page's text to `POST /v1/privacy/classify` with model `openai/privacy-filter` and store only per-page category counts in `pii.json`; the span texts SHALL never be stored or delivered.

#### Scenario: Names found
- **WHEN** classify returns `private_person` spans on page 2
- **THEN** `pii.json` counts them for page 2 and holds no span text

### Requirement: No aliasing
Every extraction and classify call SHALL be sent with `x-no-aliasing: true`, and an answer whose `model` is not the requested model SHALL fail the step.

#### Scenario: Aliased extraction
- **WHEN** the extraction answer names another model than `Qwen/Qwen3-VL-30B-A3B-Instruct`
- **THEN** the page call fails

### Requirement: Error texts
An error of a digest command or of the model client SHALL name the step and the field, and SHALL NOT quote response, model or document content.

#### Scenario: Mistyped field
- **WHEN** the digest answer has a figure whose `page` is the string `Invoice for Jane Doe`
- **THEN** the step fails saying the answer does not match the schema, and the message does not contain `Jane`

### Requirement: Digest call
`nearbee digest compose` SHALL build one request to `deepseek-ai/DeepSeek-V4.1-Flash-1017` from the page texts, the PII counts and the `feedback` param (data, never instructions), at most 900 KiB, run it through `nearbee signed-call`, and require the model's JSON to be exactly `{"summary", "entities", "figures", "open_questions"}` as the scenarios bound it; a `pii` key in the answer SHALL be refused.

#### Scenario: Invalid JSON answer
- **WHEN** the model's answer does not parse into the schema
- **THEN** the step fails and leaves no `out/result.json`

#### Scenario: Schema bounds
- **WHEN** the answer is checked
- **THEN** `summary` is a string of 1 to 2000 characters; `entities` at most 100 of `{"type": "organisation"|"document_type"|"date"|"amount", "value": 1 to 200 characters}`; `figures` at most 100 of `{"value": 1 to 100, "label": 1 to 200 characters, "page": integer 1 to 40}`; `open_questions` at most 20 strings of 1 to 500 characters; anything else fails the step

#### Scenario: Model writes PII counts
- **WHEN** the model's answer has a `pii` key
- **THEN** the step fails

#### Scenario: Feedback on a rework
- **WHEN** the `feedback` param is not empty
- **THEN** the request carries it as a separate block labelled as the buyer's change request, at most 4,000 characters

### Requirement: Digest file
Nearbee SHALL write `digest.json` as `{version: 1, summary, entities, figures, open_questions, pii, pages: {digested, skipped, truncated}, dropped_figures, dropped_personal_figures, redacted_spans}` with `pii` taken from `pii.json`, never from the model. A page counts once: a page cut to fit the request is counted in `skipped` only, and `truncated` counts digested pages only.

#### Scenario: PII from classification
- **WHEN** the digest is written
- **THEN** its `pii` equals the counts of `pii.json`

#### Scenario: Truncated page cut from the request
- **WHEN** a page marked truncated is cut to fit 900 KiB
- **THEN** it is counted in `skipped` and not in `truncated`

### Requirement: Redaction
Before `digest.json` is written, `compose` SHALL send every free-text value of the answer (summary, entity values, figure values and labels, open questions) through `POST /v1/privacy/classify` with `openai/privacy-filter` and replace each span with `[redacted: <category>]`; a figure whose value held a span SHALL be dropped and counted in `dropped_personal_figures`, and the replaced spans SHALL be counted in `redacted_spans`.

#### Scenario: Name in the summary
- **WHEN** the summary holds a personal name the classifier marks `private_person`
- **THEN** the delivered summary shows `[redacted: private_person]` in its place and neither `digest.json` nor the deliverable holds the name

#### Scenario: Phone number as a figure
- **WHEN** a figure's value is a phone number found on the cited page and the classifier marks it
- **THEN** the figure is dropped and counted in `dropped_personal_figures`

### Requirement: Redaction fails closed
A classify span outside the sent text SHALL fail the step without a digest, and `compose` SHALL refuse to run, sending nothing, when `openai/privacy-filter` is not in the hire's direct models.

#### Scenario: Span outside the text
- **WHEN** the classifier returns a span that ends past the sent text
- **THEN** the step fails and leaves no `out/digest.json`

### Requirement: Figures checked
A figure SHALL be kept only when its value, after collapsing whitespace runs to one space, is at least 2 characters long and is a substring of the cited page's stored text after the same collapsing (case kept); dropped figures SHALL be counted in the deliverable.

#### Scenario: Wrong page
- **WHEN** a figure cites page 3 but its value appears only on page 5
- **THEN** it is dropped and counted

#### Scenario: One-character value
- **WHEN** a figure's value is `1`
- **THEN** it is dropped and counted

### Requirement: Deliverable
Nearbee SHALL render `out/deliverable.md` from `digest.json` with every text escaped, no links, no images and no clickable bare URL or address, the page count, skipped and truncated pages, the PII categories without values, and `digest.json` in a fence of `max(3, longest backtick run + 1)` backticks; `out/result.json`, written last, names the Markdown and the signed call's evidence entries.

#### Scenario: Link in a document
- **WHEN** a summary sentence contains `[click](https://example.com)`
- **THEN** the Markdown shows it as escaped text, not a link

#### Scenario: Bare address
- **WHEN** a summary holds `https://x.y`, `www.x.y` or `a@b.c`
- **THEN** none of them renders as a link

#### Scenario: Bare domain
- **WHEN** a summary holds `pay-here.example`
- **THEN** its dot is escaped as `\[.\]` and no fuzzy linkifier finds a host

#### Scenario: Setext underline
- **WHEN** a summary holds a line `===` after a line of text
- **THEN** the `=` line is escaped and renders no heading
