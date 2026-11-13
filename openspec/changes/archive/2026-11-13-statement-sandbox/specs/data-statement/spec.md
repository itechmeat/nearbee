# Spec Delta

## MODIFIED Requirements

### Requirement: Where the run happens
The statement SHALL say that the run happens on the owner's self-hosted machine, which can read the data. Without a sandbox fact it SHALL say that the steps run without a sandbox and that reading files the owner's account can read and network access the bindings do not declare are not prevented. With a sandbox fact it SHALL instead describe the sandbox as the next requirement says.

#### Scenario: Attested listing
- **WHEN** the statement of an attested listing is generated
- **THEN** it still says the owner's machine can read the data

#### Scenario: No sandbox fact
- **WHEN** the facts carry no sandbox fact
- **THEN** the section text is byte for byte the text before this change

## ADDED Requirements

### Requirement: Sandbox wording
With a sandbox fact the statement SHALL say that the steps run in a container on the owner's machine with a read-only system, no added privileges and only the instance's own directories mounted, that outbound connections are limited to the listed hosts, and that hires of every listing on that machine share the container and are kept apart by working directories only.

#### Scenario: Sandboxed instance
- **WHEN** the facts carry a sandbox fact with the hosts `cloud-api.near.ai` and `github.com`
- **THEN** the statement names the container limits, lists exactly those two hosts in sorted order, says hires share the container, and does not contain "run without a sandbox"

