# Spec Delta

## ADDED Requirements

### Requirement: Direct models
A listing's `direct_models` SHALL be the only models its Nearbee commands may call from script nodes. An attested listing SHALL be refused unless every direct model is in the catalog snapshot with owner `nearai`; a plain listing SHALL be refused when a direct model is not in the snapshot.

#### Scenario: Incognito direct model
- **WHEN** an attested listing lists `anthropic/claude-sonnet-5` as a direct model
- **THEN** the listing is refused naming the model

#### Scenario: Off-catalog direct model on a plain listing
- **WHEN** a plain listing lists a model absent from the catalog snapshot
- **THEN** the listing is refused naming the model

#### Scenario: Command outside the list
- **WHEN** a digest command is asked to call a model the hire's frozen direct models do not list
- **THEN** the command fails before any request

### Requirement: Direct call route
The model routes SHALL be unknown when the apb instance environment sets `NEARAI_CLOUD_URL`, any `NEARBEE_CLOUD_` variable, `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` (any case); `nearbee list` then refuses an attested listing and the drift pass reports it drifted. Nearbee's commands SHALL call only `https://cloud-api.near.ai/v1`, without a proxy.

#### Scenario: Redirecting variable
- **WHEN** the instance environment sets `NEARAI_CLOUD_URL`
- **THEN** `nearbee list` refuses the attested listing naming unknown routes
