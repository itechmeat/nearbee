# Spec Delta

## MODIFIED Requirements

### Requirement: Stable profile names
Nearbee SHALL ship profile templates named `nearbee-own`, `nearbee-own-premium` and `nearbee-near`, and market playbooks SHALL reference executors only through these names, so that changing the executor route is a profile edit and never a playbook edit.

#### Scenario: Switching the near route
- **WHEN** the `nearbee-near` profile's executor changes from one agent to another
- **THEN** no playbook file changes

## ADDED Requirements

### Requirement: Near profile model order
The `nearbee-near` template SHALL run `deepseek-ai/DeepSeek-V4.1-Flash-1017` as its executor model and `Qwen/Qwen3.8-27B` as its only fallback, both through the `nearai` provider.

#### Scenario: Template order
- **WHEN** the template is read
- **THEN** the executor model is DeepSeek V4.1 Flash and the fallback is Qwen3.8-27B

### Requirement: Premium profile
The `nearbee-own-premium` template SHALL run the agent `claude` with the model `claude-opus-5-5` on the owner's subscription, with `environment: minimal`, an empty process environment allowlist, and no fallback to any other provider.

#### Scenario: Template contents
- **WHEN** the template is read
- **THEN** it names `claude` and `claude-opus-5-5`, has no fallbacks and holds no secret
