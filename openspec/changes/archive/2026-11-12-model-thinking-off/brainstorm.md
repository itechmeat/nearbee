# Brainstorm

## Problem

The first live runs of the fast repository audit failed at the review step:
the signed DeepSeek V4.1 Flash call did not answer within the 240-second
completion timeout, even for a 20-file repository (an 11 KB request). A
streamed copy of the same request showed why: the model spent the whole time
on reasoning text (14,500 characters in 90 seconds, no answer content). The
digest call of the document digest uses the same model and the same request
shape, so it is exposed to the same failure. Without a fix no live hire of
either example agent can be delivered.

## Related work

None in flight. The request shapes come from the archived changes
`2026-11-11-repo-audit-and-tiers` and `2026-11-11-doc-digest-and-paid-demo`;
the timeouts from `2026-11-10-market-and-first-hire` (signed call, 360 s per
invocation) and the Cloud client (240 s per exact completion).

## Doubts

1. Whether NEAR AI Cloud honours a request flag that turns reasoning off.
2. Whether the answer quality without reasoning is still usable.
3. Whether the signature still verifies with an extra request key (the
   signed text hashes the exact request bytes, so any key is covered).

## Approaches considered

1. Turn reasoning off in the request with
   `"chat_template_kwargs": {"thinking": false}`. Measured on 2026-11-12: a
   short prompt answered in 0.7 to 1.5 s with zero reasoning tokens; the
   exact failing review request answered in 34 s, `finish_reason: stop`,
   1,330 completion tokens, `reasoning_tokens: 0`, with well-formed findings
   that cite files and lines.
2. `"reasoning_effort": "low"`. Measured: reasoning still produced; no
   effect on this model.
3. `"thinking": {"type": "disabled"}`. Measured: ignored.
4. Raise the timeouts only. A streamed run of the same request was still
   reasoning after 390 s; with `max_tokens: 16384` most of the budget goes to
   reasoning, so the answer risks `finish_reason: length` as well.
5. Stream the call. Rejected by the signed-call contract (`stream: false`
   is what the signature covers).

## Decision

Approach 1 for every DeepSeek call Nearbee builds (the review and the
digest call), plus aligning the exact-completion timeout with the 360-second
invocation bound so a slow but valid answer is not cut 120 s early. The
Qwen vision extraction is an instruct model without reasoning and stays
unchanged.

## Rejected

1. `reasoning_effort` and `thinking.type`: measured to have no effect.
2. Timeouts only: the model still reasons past any practical bound.
3. Streaming: breaks the signed exact-bytes contract.

## Process

A fix change made on the owner's go-ahead of 2026-11-12. The measurements
above stood in for a brainstorm partner; the change is small and every
approach was checked against the live API. Spend for the measurements stays
inside the owner's test budget of USD 1.
