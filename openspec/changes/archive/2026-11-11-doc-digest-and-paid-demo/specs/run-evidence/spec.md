# Spec Delta

## MODIFIED Requirements

### Requirement: Which models
For an attested hire Nearbee SHALL collect one attestation report per model id of the generation's frozen evidence model set (the distinct models of every usable profile's executor, fallbacks and tiers, and the listing's direct models, snapshotted with the generation's statement), at two points: before the market start and apb start of the generation, and after its run is observed terminal and succeeded. Both points SHALL use the same frozen set.

#### Scenario: Two models
- **WHEN** the frozen set is DeepSeek V4.1 Flash and Qwen3.8-27B
- **THEN** four reports are collected for the generation, two at start and two at end

#### Scenario: Profile edited mid-run
- **WHEN** a fallback model is added to the profile while the run is active
- **THEN** the end collection still uses the frozen two-model set

#### Scenario: Agent B set
- **WHEN** a `doc-digest-private` generation starts
- **THEN** its evidence model set holds Qwen3-VL, the privacy filter and DeepSeek
