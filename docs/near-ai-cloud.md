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

### Attestation reports, measured 2026-11-11 (S4)

`GET /v1/attestation/report?model=<URL-encoded id>&signing_algo=ecdsa&nonce=<64 hex>`
answered 200 for `deepseek-ai/DeepSeek-V4.1-Flash-1017` (304,174 bytes)
and `Qwen/Qwen3.8-27B` (287,342 bytes) with an empty credit balance, so
the call is not billed. The body is a JSON object with
`gateway_attestation`, `model_attestations`, `ohttp_key_config` and
`ohttp_attestation`. A request without the bearer key also answered 200,
although the vendored skill says the key is required; Nearbee sends the
key anyway. A trimmed copy is the replay fixture
`crates/nearbee-cloud/tests/fixtures/attestation_report_live.json`.
On 2026-11-11 (S5, task 3.2) the same request with `provider=near`
answered 200 for `Qwen/Qwen3-VL-30B-A3B-Instruct` (287,645 bytes) and
`openai/privacy-filter` (287,636 bytes), each with one entry in
`model_attestations` naming the model and echoing the request nonce, so
both direct models of agent B can be held to the attested tier. Not
billed (key only).
The Verification Center opens a model with
`https://cloud.near.ai/verify?model=<URL-encoded id>` (docs.near.ai,
verification-center page, read 2026-11-11).

How Nearbee uses it (S4, `crates/nearbee-cloud`, `docs/privacy.md`
"Evidence"): Nearbee always sends the bearer `NEARAI_API_KEY`, and treats
the key as required for attested listings: `nearbee serve` with an attested
listing and no key fails every report fetch with "NEARAI_API_KEY is not
set" and sends no request, so attested hires are declined
(`evidence_unavailable`) instead of delivered without evidence; plain
listings never need the key. Each fetch sends a fresh random 32-byte hex
`nonce`, follows no redirect, reads at most 2 MiB and makes at most two
attempts (9 seconds each, 1 second apart, retried only on a timeout, a
connect error or a 5xx) within 20 seconds; a 2xx with a JSON object body
is a report. The deliverable links each model as
`https://cloud.near.ai/verify?model=<id with / as %2F>`. `NEARAI_CLOUD_URL`
overrides the API base (default `https://cloud-api.near.ai/v1`).

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

### Response signatures and classify (S5)

Read from docs.near.ai on 2026-11-11 (response-signatures,
verification-center and specialized-endpoints pages); the documented
answers are the replay fixtures in `crates/nearbee-cloud/tests/fixtures/`
(`signature_docs.json`, `classify_docs.json`, `chat_completion_docs.json`),
each with its source and read date in `note`. Not yet measured live: the
first live answers are recorded in task 7.2.

1. A `provider_tee` signature needs the completion sent with
   `x-no-aliasing: true`; Nearbee also sends `Accept-Encoding: identity`
   and refuses a compressed answer, so the stored bytes are the hashed
   bytes.
2. `GET /v1/signature/{completion id}?model=<id>&signing_algo=ecdsa`
   answers `text` (`<model>:<sha256 request>:<sha256 response>`),
   `signature` (0x plus 130 hex), `signing_address` (0x plus 40 hex),
   `signing_algo` and `signature_kind` (`provider_tee` or `gateway`; only
   `provider_tee` binds the response to a model TEE). A 404 or a 2xx with
   `error_code` means not recorded yet; Nearbee retries every 5 s for up
   to 60 s.
3. The signer is matched against the model attestations of a report
   fetched with `provider=near` before the completion. The
   response-signatures page says not to use a later report as a
   substitute for preflight evidence, and that instances of one model
   share one signing key.
4. The Verification Center takes a receipt in the URL fragment:
   `https://cloud.near.ai/verify?model=<URL-encoded id>#receipt=<base64url JSON>`
   with the keys `completionId`, `model`, `signatureKind`, `signingAlgo`,
   `signingAddress`, `signedText`, `signature`.
5. `POST /v1/privacy/classify` with `{"model": "openai/privacy-filter",
   "input": <text>}` answers `data[].spans[]` with `category`, `start`,
   `end`, `text` and `score`; Nearbee drops `text` while parsing and keeps
   only counts per category. The documented example is ASCII, so whether
   `start` and `end` count characters or bytes is not shown; Nearbee
   reads them as characters (Unicode scalar values, as a Python service
   reports them) when it redacts the digest's own text, and a span past
   the end of the sent text fails the step. To be confirmed on the first
   live answer with non-ASCII text.
7. Nearbee sends `x-no-aliasing: true` on every completion and classify
   call, not only the signed one, and refuses an extraction or classify
   answer whose `model` is not the requested one, so the model the
   statement and the evidence name is the model that served the call.
6. No completion has an idempotency key: a lost completion answer is never
   resent (one failed hire is cheaper than two billed calls).

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
