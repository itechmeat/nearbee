# NEAR AI Cloud: what Nearbee uses

Verified 2026-11-09 against `https://cloud-api.near.ai/v1/models` (copy with
pricing in `docs/reference/near-ai-cloud-models.json`) and
`https://docs.near.ai/llms.txt` pages.

## API

- OpenAI-compatible: `https://cloud-api.near.ai/v1` (chat completions, stateless
  responses, embeddings, rerank, images, audio transcription, privacy classify and
  redact). Any OpenAI-compatible client works, so apb executors that take a custom
  OpenAI-compatible provider (OpenCode, Codex custom providers; DSH natively in a
  coming apb release) can run on it with a base URL and a key.
- Credits: card or crypto in the dashboard; USD 10 via nearbuilders.org/join.
- Public `GET /v1/models` with pricing per million tokens, context length,
  features (`tools`, `structured_outputs`, `reasoning`), modalities.

## Privacy tiers of models (catalog labels)

| Tier | Where it runs | Evidence | Examples (2026-11-09) |
| --- | --- | --- | --- |
| Confidential TEE (`owned_by: nearai`) | NEAR AI's own fleet, Intel TDX + NVIDIA confidential computing | gateway and model attestation, response signatures, E2EE | `Qwen/Qwen3.8-27B` (0.04 / 1.35 USD per M), `deepseek-ai/DeepSeek-V4.1-Flash-1017` (0.3 / 1.2, 1M context), `Qwen/Qwen3-VL-30B-A3B-Instruct` (vision, image content parts only, no `tools`, 0.15 / 0.55), `zai-org/GLM-5.1-FP8`, `openai/whisper-large-v3` (STT), `openai/privacy-filter` (not a chat model: text through the dedicated `/v1/privacy/*` endpoints) |
| Other owners | `typesafe/jev-1.13` is `owned_by: typesafe`; its processing and evidence policy must be checked separately before it counts for any tier | | |
| 3P Confidential TEE (`owned_by: attested 3p`) | a third-party provider's enclave | provider's own attestation; JS SDK verifies it, Python SDK verifies the gateway only | `z-ai/glm-5.3`, `moonshotai/kimi-k3`, `deepseek/deepseek-v3.2` |
| Incognito | provider's infrastructure outside a TEE, via the NEAR AI gateway under NEAR AI's account | gateway TEE only; credentials not forwarded | `anthropic/claude-*`, `openai/gpt-*`, `google/gemini-*` |

For Nearbee's **private** listing tier only Confidential TEE models qualify.
Incognito models give "the provider does not learn who you are", not "nobody
sees the data", and must not be sold as private.

## Verification

- `GET /v1/attestation/report?model=X` (API key optional): hardware attestation
  for the gateway and the model, with the model's signing public key.
- Response signatures: `GET` signature for a completion id; verify over the
  exact request and response bytes with matching preflight evidence (docs:
  response-signatures.md). Only a `provider_tee` signature binds a response to
  a model-serving TEE; streamed responses commonly carry a gateway signature
  only. A report fetched at run start or end proves nothing about a particular
  response: per-response evidence needs the caller to hold the exact bytes.
- TLS connection binding of the gateway (tls.md); Intel Trust Authority tokens.
- Verification Center: browser UI to check a model's attestation and a reply's
  signature without code (docs: verification-center.md). Good for the demo and
  for a buyer reading a deliverable.
- SDKs with built-in verification in JavaScript, Python, Rust, Go.

## End-to-end encryption

Client encrypts each message with the model's attested public key before it
leaves the client (X25519 ECDH, HKDF-SHA256, XChaCha20-Poly1305; Ed25519 keys
converted to X25519; wire format `[eph pubkey 32][nonce 24][ciphertext+tag]`),
sends it through the gateway with encryption headers, the model TEE decrypts,
runs inference, encrypts the reply to the client's ephemeral key. The gateway
sends client-E2EE requests only to a compatible NEAR-serving path. Reference
implementation in the docs (Python, PyNaCl + cryptography).

For Nearbee this enables the **blind** mode of `docs/privacy.md`: a caller that
encrypts the brief for the attested model key, so the adapter relays ciphertext
it cannot read. It is only possible for x402 callers (programs), never for a web
buyer, and only for playbook steps where the model does the whole step.

## Other useful endpoints

- `openai/whisper-large-v3`: transcription of audio attachments inside a TEE
  (25 MB files).
- `openai/privacy-filter`, `/v1/privacy/classify`, `/v1/privacy/redact`: PII
  detection and redaction inside a TEE; a cheap intake step before any non-TEE
  model sees a brief.
- `typesafe/jev-1.13`: a decision model (System One) usable by apb's decision
  providers; lets `apb decide` run on NEAR for intake and routing decisions.
- Fusion (server-side multi-model deliberation), web search tools, prompt
  caching with discounted cache reads.

## Gaps

1. No TTS.
2. The list of models that support client E2EE is not published as a flag in
   `/v1/models`; check per model with the attestation endpoint.
3. Whether apb's current OpenCode or Codex custom-provider path handles the
   NEAR AI base URL with reasoning models cleanly is untested (`docs/plan.md`).

## Smoke runs through apb (S1)

Setup verified 2026-11-09: OpenCode 2.2.5 with the custom provider `nearai`
in the dedicated workspace's `opencode.json` (`docs/apb.md`, "OpenCode on
NEAR AI Cloud") lists `nearai/Qwen/Qwen3.8-27B` and
`nearai/deepseek-ai/DeepSeek-V4.1-Flash-1017`. The profiles are
`playbooks/profiles/nearbee-near.yaml` (Qwen, DeepSeek fallback) and the
smoke-only `nearbee-near-deepseek.yaml`.

| Model | apb | OpenCode | Run | Status | Manifest | Usage | Cost |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `Qwen/Qwen3.8-27B` | 1.0.0-beta.6 | 2.2.5 | not run | blocked: the account balance is empty (O-3, 2026-11-09) | | | |
| `deepseek-ai/DeepSeek-V4.1-Flash-1017` | 1.0.0-beta.6 | 2.2.5 | not run | blocked: same | | | |

To run them once the credits are on the account and the owner allows the
spend: `scripts/apb-instance.sh allow-billed` (apb otherwise refuses every
OpenCode-on-`nearai` start with `billing_requires_confirmation`), then `NEARBEE_LIVE=1 cargo test -p nearbee-apb --test live --
--ignored live_smoke_qwen --nocapture` and the same with
`live_smoke_deepseek` (budget USD 0.50 each).
