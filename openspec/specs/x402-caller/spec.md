# x402-caller Specification

## Purpose
Defines the demo caller that hires a market agent agent-to-agent and pays with x402, and what it must prove.

## Requirements

### Requirement: Payment construction
The caller SHALL read the agent card, send an unpaid `SendMessage`, take verbatim the 402 `accepts[]` row whose `network` the caller holds funds on, sign one NEP-366 delegate action of `ft_transfer` on the row's asset to its `payTo` for its amount with 1 yoctoNEAR and 30 Tgas, valid for the row's `maxTimeoutSeconds`, and send it in `X-PAYMENT` with `returnImmediately: true`.

#### Scenario: Row reused verbatim
- **WHEN** the 402 row is received
- **THEN** the `accepted` field of the payment payload deep-equals it after JSON parsing

### Requirement: Local verification
Before sending, the caller SHALL verify its own signed delegate: signer and public key match the caller account's key on chain, receiver is the asset contract, the single action is the expected `ft_transfer`, and the block window is in the future; the check result SHALL be printed.

#### Scenario: Wrong key
- **WHEN** the configured private key does not belong to the caller account
- **THEN** the caller stops before sending and says so

### Requirement: No secrets in output
The caller SHALL read the account id and private key from the environment and never print the key.

#### Scenario: Missing key
- **WHEN** `NEAR_CALLER_PRIVATE_KEY` is unset
- **THEN** the caller exits non-zero naming the variable
