# signed-call Specification

## Purpose
Defines the signed model call: one non-streamed NEAR AI Cloud chat completion sent from exact request bytes, kept with its exact response bytes and its `provider_tee` signature, so a deliverable can prove that this inference ran in a model TEE.

## Requirements

### Requirement: Exact bytes
`nearbee signed-call --request <file> --out <dir>` SHALL send the request file's bytes unchanged as the body of `POST https://cloud-api.near.ai/v1/chat/completions` with headers `x-no-aliasing: true` and `Accept-Encoding: identity`, and SHALL refuse a request whose JSON lacks `"stream": false` or whose `model` is not in the hire's frozen direct models and a canonical catalog id owned by `nearai`.

#### Scenario: Streaming request
- **WHEN** the request file sets `"stream": true`
- **THEN** the command fails before sending anything

#### Scenario: Model not allowed
- **WHEN** the request's model is not in the `direct_models` run param
- **THEN** the command fails before sending anything

### Requirement: Response kept before parsing
The command SHALL write the response body bytes to `<dir>/response.bin` atomically (temp file, sync, rename) before parsing them, and SHALL refuse a response with a `Content-Encoding` header present and not `identity`, writing phase `refused` with reason `compressed`.

#### Scenario: Compressed answer
- **WHEN** the answer carries `Content-Encoding: gzip`
- **THEN** the command fails and the state records `refused` with reason `compressed`

#### Scenario: No Content-Encoding
- **WHEN** the answer carries no `Content-Encoding` header
- **THEN** the answer is accepted

### Requirement: Preflight model report
Before the completion the command SHALL fetch the model's attestation report (`provider=near`, `signing_algo=ecdsa`, fresh nonce) and store it; a missing or empty `model_attestations`, or one in which no attestation carries a `signing_address`, SHALL fail the step before the completion is sent.

#### Scenario: No model attestation
- **WHEN** the report has an empty `model_attestations`
- **THEN** no completion is sent

#### Scenario: No signing address
- **WHEN** no model attestation of the report carries a `signing_address`
- **THEN** no completion is sent

### Requirement: Signature
After the completion the command SHALL fetch `GET /v1/signature/{completion id}?model=<id>&signing_algo=ecdsa`, retrying a 404 or a 2xx carrying `error_code` within the bounds, and SHALL accept only `signature_kind: provider_tee` with `text` equal to `<model>:<sha256 of request bytes>:<sha256 of response bytes>` and a `signing_address` equal, case-insensitively, to at least one model attestation of the preflight report; a report fetched after the completion SHALL NOT replace it.

#### Scenario: Gateway signature
- **WHEN** the signature answer has `signature_kind: gateway`
- **THEN** the step fails saying the response is not bound to a model TEE

#### Scenario: Signer not in the report
- **WHEN** the signing address matches no model attestation of the preflight report
- **THEN** the step fails and no further report is fetched

#### Scenario: Instances sharing a key
- **WHEN** two model attestations of the preflight report carry the signing address (instances of one model share one signing key)
- **THEN** the signature is accepted

#### Scenario: Address case
- **WHEN** the signature's address differs from the report's only in letter case
- **THEN** the signature is accepted

#### Scenario: Retry window
- **WHEN** the signature stays unavailable
- **THEN** the fetch is retried for at most 60 seconds or until the invocation's deadline, then the step fails leaving `responded`

#### Scenario: Signature after two 404s
- **WHEN** the signature fetch answers 404 twice and then the signature
- **THEN** the signature is accepted

### Requirement: Completion id format
The command SHALL require the completion `id` to match `[A-Za-z0-9_-]{1,128}` before it is used in the signature URL; any other id SHALL fail the step.

#### Scenario: Id with a slash
- **WHEN** the completion id contains `/`
- **THEN** the step fails without fetching a signature

### Requirement: No resend
The command SHALL keep a state file in `<dir>` recording `preflight`, `sending`, `responded`, `refused` and `signed`. `responded`, or `sending` with a stored response, SHALL resume at the signature fetch; `sending` without a stored response SHALL fail without sending again; `refused`, written after a non-2xx answer, a compressed answer or a truncated answer, SHALL fail again without a request; `signed` SHALL exit 0 without network calls.

#### Scenario: Retry after a lost answer
- **WHEN** the previous attempt stopped after writing `sending` and before any response
- **THEN** the command fails saying the completion outcome is unknown and sends nothing

#### Scenario: Retry after the completion
- **WHEN** the previous attempt stored the response and stopped before the signature
- **THEN** the command fetches only the signature

#### Scenario: Refused completion
- **WHEN** the completion answers 402 and the step is retried
- **THEN** one completion request was made in total and both attempts fail naming the status

#### Scenario: Rework
- **WHEN** a rework runs after a delivered generation
- **THEN** a new signed call is made under the new execution's directory and the earlier one is not read

### Requirement: Bounds
The command SHALL refuse a request without `max_tokens` or with `max_tokens` above 16,384, a request file over 1 MiB, and a response over 4 MiB; an answer whose `finish_reason` is not `stop` SHALL fail as cut at `max_tokens`; one invocation SHALL be bounded by 360 seconds, and a resumed invocation gets a fresh bound.

#### Scenario: No token bound
- **WHEN** the request has no `max_tokens`
- **THEN** the command fails before sending

#### Scenario: Truncated answer
- **WHEN** the answer's `finish_reason` is `length`
- **THEN** the step fails saying the answer was cut at `max_tokens`

### Requirement: Completion wait
Inside the invocation's bound the completion wait SHALL use the time left minus a 30-second signature reserve, with no shorter fixed timeout, and the call SHALL fail before sending when nothing is left after the reserve.

#### Scenario: Slow valid answer
- **WHEN** the completion answers inside the bound minus the signature reserve
- **THEN** the answer is accepted and the signature fetch still has the reserve

#### Scenario: No time left to send
- **WHEN** less than the signature reserve is left before the completion is sent
- **THEN** the command fails without sending and the state stays `preflight`

### Requirement: Evidence entries
On success the command SHALL write `<dir>/signature.json` for the manifest entry `{kind: tee_signature, visibility: public}`, and beside it, in the same directory, `request.bin` (`kind: tee_request`), `response.bin` (`kind: tee_response`) and `report.json` (`kind: tee_model_report`), each for a `visibility: private` entry.

#### Scenario: Entries written
- **WHEN** a signed call succeeds
- **THEN** `signature.json` is a JSON object with exactly `version` (1), `completion_id`, `model`, `signature_kind`, `signing_algo`, `signing_address`, `signed_text`, `signature`, `request_sha256` and `response_sha256`, and the private files exist beside it
