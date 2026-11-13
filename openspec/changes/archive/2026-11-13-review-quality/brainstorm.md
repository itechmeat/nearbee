# Brainstorm

## Problem

The first paid hire of the fast audit (2026-11-13, `itechmeat/nearbee`) was
delivered with no findings: the pack filled the 768 KiB cap (49 files,
213,396 prompt tokens) and DeepSeek V4.1 Flash with reasoning off answered
`{"version": 1, "findings": []}` in 13 tokens. A paid hire must not return
an empty audit because the input was too large for the model to work on.

## Related work

`model-thinking-off` (reasoning off) made the calls finish; this change
keeps that and fixes what the model does with a large pack.

## Doubts

1. Whether a smaller pack is enough, or the answer format also fails.
2. Whether NEAR AI Cloud honours `response_format` for this model (the S6
   review dropped it as unverified).

## Approaches considered

1. Smaller pack. Measured on the same request cut to 12 files (217,060
   characters, 61,374 prompt tokens): 6 findings, but the answer was
   invalid JSON (an unescaped quote at character 4,844), which the step
   would refuse as `review_unparsed`.
2. Smaller pack plus `response_format: {"type": "json_object"}`. Measured on
   the same cut: valid JSON, 9 findings, 3,173 completion tokens, 49 s.
3. Reasoning back on with a larger bound. Rejected: the model reasoned past
   six minutes without an answer (`model-thinking-off`).

## Decision

Approach 2: the review request cap drops from 768 KiB to 256 KiB, and both
the review and the digest request carry `response_format: {"type":
"json_object"}`. The omitted files are already listed in the deliverable's
coverage section, so the buyer sees what was not reviewed.

## Rejected

1. Pack size only: measured invalid JSON.
2. Reasoning on: does not finish.

## Process

A fix change on the owner's go-ahead ("fix the project until everything
works as planned", 2026-11-12); the two measurements above stood in for a
brainstorm partner. Spend: about USD 0.04 for the two test calls.
