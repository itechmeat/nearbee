# Playbooks and profiles

Templates Nearbee installs into its own apb workspace with
`scripts/apb-instance.sh install` (see `docs/apb.md`).

## Profiles (`profiles/`)

| File | Status | Executor |
| --- | --- | --- |
| `nearbee-own.yaml` | supported | the owner's own subscription (Claude Haiku); spends no NEAR AI Cloud credit |
| `nearbee-own-premium.yaml` | supported | the owner's own Claude subscription on Claude Opus; no fallback, an empty process environment allowlist; the premium listing's profile |
| `nearbee-near.yaml` | supported | OpenCode on NEAR AI Cloud Confidential TEE models (`owned_by: nearai`): DeepSeek-V4.1-Flash, falling back to Qwen3.8-27B |
| `nearbee-near-deepseek.yaml` | smoke only | DeepSeek first, so the smoke run measures that model; market playbooks never name it |
| `nearbee-near-vision.md` | **unsupported** | placeholder; the TEE vision model has no tool calls, see the file |

Market playbooks name executors only through `nearbee-own`,
`nearbee-own-premium` and `nearbee-near`, so switching a route (for example to apb's native NEAR AI
Cloud executor) is a profile edit, never a playbook edit. No profile holds a
key: `NEARAI_API_KEY` reaches the executors from the environment of
Nearbee's apb instance.

`install` also writes `decisions: { enabled: false }` into the workspace's
`.apb/config.yaml`: an attested listing refuses decisions that are on or
unknown (`docs/privacy.md`).

## Agent A (`repo-audit/`)

`playbook.yaml` (id `repo-audit`) runs `fetch` (script `scripts/fetch.sh`,
`nearbee audit fetch`), `audit` (the agent writes `audit/candidates.json`),
`finish` (script `scripts/finish.sh`, `nearbee audit finish`) and `done`.
The scripts run `${NEARBEE_BIN:-nearbee}`; `scripts/apb-instance.sh start`
sets `NEARBEE_BIN` in the instance's environment. The source's
`defaults.profile` is `nearbee-own`; each listing file replaces it with its
own `profile` when `nearbee list` materialises it:

| Listing | Tier | Profile | Price |
| --- | --- | --- | --- |
| `listing.repo-audit-fast.yaml` | attested | `nearbee-near` | 0.50 |
| `listing.repo-audit-premium.yaml` | plain | `nearbee-own-premium` | 2.00 |

Both carry `intake_check: github_repo`. The repo-audit playbook is
installed by `nearbee list`, not by `scripts/apb-instance.sh install`.
`docs/showcase-agents.md` describes the agent as built.

## Workspace files (`workspace/`)

`opencode.json` goes to the root of Nearbee's apb workspace: the OpenCode
provider `nearai` (NEAR AI Cloud's OpenAI-compatible API) with the two models
the near profiles name. Its key is `{env:NEARAI_API_KEY}`, never a value.

## Smoke playbooks (`smoke/`)

`smoke-own`, `smoke-qwen` and `smoke-deepseek` are one synthetic playbook in
three copies (since S4 `nearbee-near` runs DeepSeek first, so `smoke-qwen`
starts on DeepSeek too and reaches Qwen only as the fallback) that differ only in `id`, `name` and `defaults.profile` (the
REST start takes no profile override). An agent writes a three-line note
about a synthetic `topic` into `out/deliverable.md`; the script
`scripts/manifest.sh` writes the result manifest `out/result.json` with the
file's real size and SHA-256. `hire_id` and `execution_id` are search params.
