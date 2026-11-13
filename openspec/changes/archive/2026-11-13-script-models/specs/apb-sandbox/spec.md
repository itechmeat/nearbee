# Spec Delta

## ADDED Requirements

### Requirement: Claude Code CLI in the image
The sandbox image SHALL contain the Claude Code CLI at a pinned version with its auto-updater and non-essential traffic disabled, so a script step can run `claude -p` with a token passed through `NEARBEE_APB_ENV_VARS`.

#### Scenario: CLI present
- **WHEN** the image is built from `sandbox/Dockerfile`
- **THEN** `claude --version` inside it prints the pinned version

### Requirement: DeepSeek Harness in the image
The sandbox image SHALL contain DeepSeek Harness (`dsh`) at a pinned version, and the egress allowlist SHALL include `api.z.ai`, so a script step can run a dsh review with `ZAI_API_KEY` passed through `NEARBEE_APB_ENV_VARS`.

#### Scenario: dsh present
- **WHEN** the image is built from `sandbox/Dockerfile`
- **THEN** `dsh --version` inside it prints the pinned version
