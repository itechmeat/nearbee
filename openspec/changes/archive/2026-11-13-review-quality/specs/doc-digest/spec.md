# Spec Delta

## MODIFIED Requirements

### Requirement: Digest call
`nearbee digest compose` SHALL build one request to `deepseek-ai/DeepSeek-V4.1-Flash-1017` with `chat_template_kwargs: {"thinking": false}` and `response_format: {"type": "json_object"}` from the page texts, the PII counts and the `feedback` param (data, never instructions), at most 900 KiB, run it through `nearbee signed-call`, and require the model's JSON to be exactly `{"summary", "entities", "figures", "open_questions"}` as the scenarios bound it; a `pii` key in the answer SHALL be refused.

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

#### Scenario: Reasoning off
- **WHEN** the digest request is built
- **THEN** its `chat_template_kwargs` is exactly `{"thinking": false}` and its `response_format` is exactly `{"type": "json_object"}`
