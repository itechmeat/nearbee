# Spec Delta

## ADDED Requirements

### Requirement: Direct models in the statement
The statement SHALL list each direct model as called directly by Nearbee commands in script steps, with where it runs, and SHALL name the signed step when the listing's `signed_models` is not empty; the fingerprint SHALL cover the direct and signed models in order and SHALL be unchanged for a listing without them.

#### Scenario: Agent B statement
- **WHEN** the statement of `doc-digest-private` is generated
- **THEN** it names Qwen3-VL, the privacy filter and DeepSeek as direct NEAR AI Cloud TEE calls and says the DeepSeek call carries a response signature

#### Scenario: No drift from direct models
- **WHEN** the drift pass inspects a listing with direct models whose apb facts are unchanged
- **THEN** the listing is clean

#### Scenario: S4 fingerprint unchanged
- **WHEN** a listing without direct models is fingerprinted
- **THEN** the fingerprint equals the one S4 computed for the same facts
