# Spec Delta

## MODIFIED Requirements

### Requirement: Bounds
The command SHALL refuse a request without `max_tokens` or with `max_tokens` above 16,384, a request file over 1 MiB, and a response over 4 MiB; an answer whose `finish_reason` is not `stop` SHALL fail as cut at `max_tokens`; one invocation SHALL be bounded by 360 seconds, and a resumed invocation gets a fresh bound.

#### Scenario: No token bound
- **WHEN** the request has no `max_tokens`
- **THEN** the command fails before sending

#### Scenario: Truncated answer
- **WHEN** the answer's `finish_reason` is `length`
- **THEN** the step fails saying the answer was cut at `max_tokens`

## ADDED Requirements

### Requirement: Completion wait
Inside the invocation's bound the completion wait SHALL use the time left minus a 30-second signature reserve, with no shorter fixed timeout, and the call SHALL fail before sending when nothing is left after the reserve.

#### Scenario: Slow valid answer
- **WHEN** the completion answers inside the bound minus the signature reserve
- **THEN** the answer is accepted and the signature fetch still has the reserve

#### Scenario: No time left to send
- **WHEN** less than the signature reserve is left before the completion is sent
- **THEN** the command fails without sending and the state stays `preflight`
