# run-evidence Specification

## Purpose
Defines the endpoint attestation evidence an attested hire collects and discloses, and how a hire behaves when the evidence cannot be collected, so an attested deliverable never ships without what it promises.

## Requirements

### Requirement: Which models
For an attested hire Nearbee SHALL collect one attestation report per model id of the generation's frozen evidence model set (the distinct models of every usable profile's executor, fallbacks and tiers, and the listing's direct models, snapshotted with the generation's statement), at two points: before the market start and apb start of the generation, and after its run is observed terminal and succeeded. Both points SHALL use the same frozen set.

#### Scenario: Two models
- **WHEN** the frozen set is DeepSeek V4.1 Flash and Qwen3.8-27B
- **THEN** four reports are collected for the generation, two at start and two at end

#### Scenario: Profile edited mid-run
- **WHEN** a fallback model is added to the profile while the run is active
- **THEN** the end collection still uses the frozen two-model set

#### Scenario: Agent B set
- **WHEN** a `doc-digest-private` generation starts
- **THEN** its evidence model set holds Qwen3-VL, the privacy filter and DeepSeek

### Requirement: The report request
Each report SHALL be fetched with `GET https://cloud-api.near.ai/v1/attestation/report` with the URL-encoded `model`, `signing_algo=ecdsa` and a fresh random 32-byte hex `nonce`, bearer authentication from the configured `NEARAI_API_KEY`, no redirects, a 2 MiB response cap, and at most 2 attempts within a 20-second bound for the whole fetch. A 2xx with a JSON object body is a report; anything else is a failure.

#### Scenario: Oversized body
- **WHEN** the response exceeds 2 MiB
- **THEN** the fetch fails without reading past the cap

#### Scenario: No key
- **WHEN** no `NEARAI_API_KEY` is configured
- **THEN** the fetch fails with that reason and no request is sent; plain listings never need the key

### Requirement: One fetch per round
Evidence collection SHALL be a long step of the serve round, like a scope check or a download: at most one report fetch per round, cut off at the earliest deadline of any active hire.

#### Scenario: Other hire's deadline
- **WHEN** another hire's abort is due in 5 seconds
- **THEN** no report fetch starts in this round

### Requirement: Private storage
The raw response bytes, nonce, model id, fetch time and SHA-256 of each report SHALL be stored as a flat snapshot file of the generation, outside the worktree, then journaled as an `evidence_collected` record with phase, model and hash. A failed fetch SHALL be journaled as `evidence_failed` with phase, model and a short reason. The key SHALL never be stored or logged.

#### Scenario: Crash between file and record
- **WHEN** serve stops after writing a report file and before its journal record
- **THEN** on restart the file's stored hash is checked and the record is written without fetching again

### Requirement: Fail closed
On the third `evidence_failed` record of a phase in a generation, Nearbee SHALL decline the hire at once: `rejected` when the generation is the first and the hire was not started on the market, `failed` otherwise. An attested deliverable SHALL never be submitted without both collections.

#### Scenario: Cloud down at start
- **WHEN** three start fetches fail for a first-time hire not started on the market
- **THEN** the hire is declined `rejected` with attention `evidence_unavailable` and neither the market start nor an apb run happens

#### Scenario: Cloud down at end
- **WHEN** the run succeeded but three end fetches fail
- **THEN** the hire is declined `failed` and nothing is submitted

### Requirement: Recovery
Stored reports SHALL be reused after their hash is checked and never fetched again to stand in for them. Start reports SHALL be fetched only while the generation has no `run_start` intent; a generation whose `run_start` intent carries the S4 references but whose statement or start evidence is missing or does not match SHALL fail closed (`failed`), never deliver without the footer or the start reports.

#### Scenario: Start evidence lost after the start
- **WHEN** a start report file is missing after the `run_start` intent was journaled
- **THEN** the hire is declined `failed` with attention `evidence_missing` and no start report is fetched

### Requirement: Disclosure
An attested deliverable SHALL carry, before the data statement, a section `Endpoint attestation disclosure` listing per model the model id, the start and end report SHA-256 and fetch times, and a link `https://cloud.near.ai/verify?model=<URL-encoded id>`; and per node attempt of the delivered run the model apb requested (or `not recorded`) and the reported model, `none reported`, or `mixed`.

#### Scenario: Reported model missing
- **WHEN** apb's attempt record has no reported model
- **THEN** the line says the requested model and `none reported`, never the profile's model in its place

### Requirement: Disclosure wording
The disclosure SHALL say that Nearbee collected the reports and did not verify them, and that they show which endpoints attest as TEEs without binding this job's responses to a TEE.

#### Scenario: Wording present
- **WHEN** an attested deliverable is assembled
- **THEN** its disclosure section contains both sentences
