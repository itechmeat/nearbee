# nearbee-near-vision: unsupported placeholder

Status: **unsupported**. Not shipped as an agent profile and not offered for
any listing.

Reason: the only Confidential TEE vision model on NEAR AI Cloud,
`Qwen/Qwen3-VL-30B-A3B-Instruct` (`owned_by: nearai`), accepts image content
parts but does not support tool calls (its catalog `features` lack `tools`;
`docs/reference/near-ai-cloud-models.json`, fetched 2026-11-09). An apb
`agent_task` executor (OpenCode, Codex) drives the model through tool calls,
so the model cannot run an agent step. Vision inference is planned as a
direct call from a playbook step in stage S5 (`docs/stages.md`), not as an
executor profile.

When it becomes possible (a vision model with `tools`, or apb running vision
calls natively), the profile would look like this, with every model owned by
`nearai`:

```yaml
name: nearbee-near-vision
description: Nearbee vision steps on NEAR AI Cloud TEE models (not supported yet)
executor:
  agent: opencode
  model: nearai/Qwen/Qwen3-VL-30B-A3B-Instruct
soul: any
environment: minimal
process_env:
  policy: allowlist
  allow: [NEARAI_API_KEY]
```
