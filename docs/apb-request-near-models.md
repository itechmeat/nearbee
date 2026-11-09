# Request to apb: first-class NEAR AI Cloud models

Sent to the apb project on 2026-11-09. The apb project writes its own OpenSpec
change from this; Nearbee does not implement it and does not wait for it.

## Why

Nearbee (this repository) sells apb playbooks on the NEAR AI Agent Market. Its
cheap and private listings run playbook steps on NEAR AI Cloud models,
especially the Confidential TEE ones. Today apb knows nothing about NEAR AI
Cloud: a profile can reach it only through an executor CLI with a hand-written
custom OpenAI-compatible provider (OpenCode `opencode.json`, or Codex
`[model_providers.nearai]`). apb then cannot list the models, validate a
profile that names one, price its tokens, tell which models are private, or
use NEAR for `apb decide`. Nearbee works around each gap; apb owning them is
better for every apb user, not only Nearbee.

Facts about NEAR AI Cloud: `docs/near-ai-cloud.md` here, the catalog copy in
`docs/reference/near-ai-cloud-models.json` (from the public
`GET https://cloud-api.near.ai/v1/models`, no key needed: id, `owned_by`,
pricing per million tokens, context length, features `tools`,
`structured_outputs`, `reasoning`, modalities). Docs index:
`https://docs.near.ai/llms.txt`.

## What apb should understand, in priority order

1. **NEAR AI Cloud as a provider in the agent catalog.** The model list for
   the executors that can reach an OpenAI-compatible endpoint comes from the
   live `/v1/models` (cached like other probes, with the embedded copy as a
   fallback), so `GET /api/models`, the dashboard profile editor,
   `apb validate` and `profile_write` know ids such as `Qwen/Qwen3.8-27B`,
   `deepseek-ai/DeepSeek-V4.1-Flash-1017`, `Qwen/Qwen3-VL-30B-A3B-Instruct`.
2. **A profile that names a NEAR model runs without hand-written executor
   config.** apb supplies the provider definition (base URL
   `https://cloud-api.near.ai/v1`, the key from one environment variable such
   as `NEARAI_API_KEY`, never stored in a profile) to the executor it spawns,
   for at least one executor: OpenCode, or apb's own native executor if that
   is the planned path. Reasoning models and models without `tools` (the
   Qwen3-VL vision model) behave predictably: a clear validation error for a
   node that needs tools on a model without them.
3. **Prices and usage.** Token usage of NEAR calls is reported and costed from
   the catalog prices, so stats, estimates and the run budget work for NEAR
   profiles (today a run with no reported cost makes the budget fall back to
   `on_no_estimate`).
4. **Privacy class of a model as data.** Each NEAR model carries its class
   from the catalog: Confidential TEE (`owned_by: nearai`), third-party TEE
   (`owned_by: attested 3p`), Incognito (provider models through the gateway,
   not private), other owners (for example `typesafe/jev-1.13`, unknown until
   checked). It is exposed in `/api/models` and in the resolved profile
   (executor and every fallback and tier), so a caller can check that a
   profile only ever reaches Confidential TEE models. A fallback that leaves
   the class silently is the case to make visible.
5. **NEAR as a decision provider** for `apb decide` through the existing
   OpenAI-compatible path, with the same privacy class reported for the
   chosen model.
6. **Run-level evidence (smaller, optional).** For a run that used NEAR TEE
   models, record in the run's evidence the attestation report reference
   (`GET /v1/attestation/report?model=<id>`) fetched for each model used, so
   a report can show it. Per-response signatures need the exact request and
   response bytes and are out of scope unless apb's native executor makes
   them cheap.

## What Nearbee needs and when

1. Nearbee uses whatever exists on the day. Items 1 to 4 are the ones Nearbee
   would switch to if they are released by 2026-11-12 (its stage S4 builds
   the privacy-tier validator on them); after that they still matter for apb
   users and for Nearbee after the hackathon.
2. Nearbee drives apb over the REST API of `apb dashboard`, so the shape that
   matters is `GET /api/models`, `GET /api/profiles/{name}` and the run's
   usage and evidence in `GET /api/runs/{id}`.

## Not part of this request

The other apb gaps Nearbee lists in its `docs/apb.md` ("Things apb may
need": the external-buyer-data flag, a result contract, a machine estimate of
an instruction, a generic HTTP notify connector) are separate and are not
asked for here.
