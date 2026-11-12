# Spec Delta

## MODIFIED Requirements

### Requirement: Review call
The review SHALL make one signed call through `nearbee signed-call`'s code with the request `stream: false`, `max_tokens: 16384` and `chat_template_kwargs: {"thinking": false}`, a system message with the step's rules and a user message holding the brief, the feedback and the pack as labelled data blocks; its state SHALL live in `audit/signed/<execution_id>/`.

#### Scenario: Rework
- **WHEN** a rework runs the review with a new execution id
- **THEN** it makes a new signed call and does not reuse the earlier answer

#### Scenario: Reasoning off
- **WHEN** the review request is built
- **THEN** its JSON has exactly the keys `model`, `stream`, `max_tokens`, `chat_template_kwargs` and `messages`, and `chat_template_kwargs` is exactly `{"thinking": false}`
