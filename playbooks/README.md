# Playbooks and profiles

Templates Nearbee installs into its own apb workspace with
`scripts/apb-instance.sh install` (see `docs/apb.md`).

## Profiles (`profiles/`)

| File | Status | Executor |
| --- | --- | --- |
| `nearbee-own.yaml` | supported | the owner's own subscription (Claude Haiku); spends no NEAR AI Cloud credit |
| `nearbee-near.yaml` | supported | OpenCode on NEAR AI Cloud Confidential TEE models (`owned_by: nearai`): Qwen3.8-27B, falling back to DeepSeek-V4.1-Flash |
| `nearbee-near-deepseek.yaml` | smoke only | DeepSeek first, so the smoke run measures that model; market playbooks never name it |
| `nearbee-near-vision.md` | **unsupported** | placeholder; the TEE vision model has no tool calls, see the file |

Market playbooks name executors only through `nearbee-own` and
`nearbee-near`, so switching a route (for example to apb's native NEAR AI
Cloud executor) is a profile edit, never a playbook edit. No profile holds a
key: `NEARAI_API_KEY` reaches the executors from the environment of
Nearbee's apb instance.

## Workspace files (`workspace/`)

`opencode.json` goes to the root of Nearbee's apb workspace: the OpenCode
provider `nearai` (NEAR AI Cloud's OpenAI-compatible API) with the two models
the near profiles name. Its key is `{env:NEARAI_API_KEY}`, never a value.

## Smoke playbooks (`smoke/`)

`smoke-own`, `smoke-qwen` and `smoke-deepseek` are one synthetic playbook in
three copies that differ only in `id`, `name` and `defaults.profile` (the
REST start takes no profile override). An agent writes a three-line note
about a synthetic `topic` into `out/deliverable.md`; the script
`scripts/manifest.sh` writes the result manifest `out/result.json` with the
file's real size and SHA-256. `hire_id` and `execution_id` are search params.
