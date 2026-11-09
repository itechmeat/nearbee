# Open questions

Numbered; answered items move to `docs/decisions.md` or the fitting doc with
the date.

## For the owner

Answered 2026-11-09: stack (D-003 Rust), privacy promise (D-008 signed digest
step), pricing (D-018 owner-set prices, optional limits), example agents stay
as written. Accounts, keys and go-aheads moved to `docs/owner-checklist.md`
(O-1 to O-10) and are requested at the plan step that needs them.

1. Retention window after acceptance: 7 days assumed until the owner says otherwise.

## To verify against the live systems

10. Market: listings per owner, review before going live, worker REST rate
    limits, `input_schema` rendering for web buyers, the exact `files` upload
    payloads and limits, `verifiableOffered` semantics, the refund-policy field
    of a listing (Job Terms 5.1) in the API.
11. NEAR AI Cloud: OpenCode custom provider with `DeepSeek-V4.1-Flash-1017`
    (reasoning) inside apb; which models expose `provider_tee` response
    signatures on non-streamed calls; `typesafe/jev-1.13` processing policy and
    reachability through apb's decision providers.
12. apb: the exact `RunDetail` fields for node outputs and `outputs.files`;
    `apb runs --field` on a `search: true` param from the REST-started run;
    deleting a run's artifacts by run id; which `.apb/config.yaml` settings turn
    off discover, memory, corrections and judge-example capture; whether a
    loopback dashboard without keys accepts Nearbee's calls.
13. x402 on mainnet: does the relayer accept a function-call access key limited
    to the USDC contract, or must the delegate be signed with a full-access key;
    does the Rust `near-api` crate build and sign a `SignedDelegateAction`
    conveniently, or does the demo caller fall back to `near-api-js`.
14. DSH native NEAR AI Cloud support in apb: release date; until then OpenCode.

## Product questions parked (after the hackathon)

15. Blind tier: an encrypted-call protocol for the relay script, ciphertext
    size and token limits, key freshness, retries, encrypted delivery; semantic
    intake is impossible on ciphertext, so intake becomes size-only.
16. Enforced privacy boundary: a dedicated execution environment with egress
    control for attested runs.
17. Board bidding with computed prices.
18. Buyer questions during a run for web and MCP buyers (request correlation
    with apb's `request` seq).
19. Webhooks as a push channel (owner-configured; fast `2xx`, intake async).
20. Anchoring hire-journal hashes on NEAR.
21. A playbook step that hires other market agents with x402 (`swarms` story).
