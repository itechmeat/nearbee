# executor-profiles Specification

## Purpose
Defines the apb profile templates Nearbee ships for running market playbooks on the owner's subscriptions or on NEAR AI Cloud models.

## Requirements

### Requirement: Stable profile names
Nearbee SHALL ship profile templates named `nearbee-own`, `nearbee-own-premium` and `nearbee-near`, and market playbooks SHALL reference executors only through these names, so that changing the executor route is a profile edit and never a playbook edit.

#### Scenario: Switching the near route
- **WHEN** the `nearbee-near` profile's executor changes from one agent to another
- **THEN** no playbook file changes

### Requirement: Near profile stays on Confidential TEE models
The `nearbee-near` template SHALL name only NEAR AI Cloud models whose catalog owner is `nearai`, in its executor, every fallback and every tier, and SHALL read the key from `NEARAI_API_KEY` without storing it.

#### Scenario: Template contents
- **WHEN** the template is read
- **THEN** every model it names is a `nearai`-owned catalog id and no secret value appears in it

### Requirement: Vision template marked unsupported
A `nearbee-near-vision` template SHALL NOT be offered as a working agent profile while the vision model lacks tool calls; it SHALL be shipped only as a documented placeholder marked unsupported.

#### Scenario: Listing the templates
- **WHEN** an operator lists Nearbee's profile templates
- **THEN** the vision template is marked unsupported with the reason

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
