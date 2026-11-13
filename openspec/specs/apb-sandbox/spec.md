# apb-sandbox Specification

## Purpose
Runs Nearbee's apb instance inside a container with the owner's tools, no access to the owner's home, and outbound traffic limited to an allowlist of TLS server names, so a buyer's brief cannot reach the owner's files or arbitrary hosts.

## Requirements

### Requirement: Sandbox start
With `NEARBEE_APB_SANDBOX=1`, `scripts/apb-instance.sh start` SHALL run the apb dashboard inside the `nearbee-sandbox` image under the same systemd user unit `nearbee-apb`, with every `Environment=` entry of the unit that the container receives having the same value inside it, and the image's own environment holding no `OPENCODE_`, proxy or `NEARAI_CLOUD_URL` variable, so the tier check reads what the container gets.

#### Scenario: Health through the gateway
- **WHEN** the sandboxed instance starts
- **THEN** `GET http://127.0.0.1:7322/api/health` answers 200 with the API key from `APB_API_KEY`

#### Scenario: Same environment
- **WHEN** `systemctl --user show nearbee-apb -p Environment` is read
- **THEN** every entry it lists that the container receives has the same value inside the container, and neither it nor `docker image inspect` shows an `OPENCODE_`, proxy or `NEARAI_CLOUD_URL` variable

### Requirement: Container limits
The container SHALL run as the owner's uid and gid with a read-only root filesystem, all capabilities dropped, `no-new-privileges`, and limits on processes, memory and CPU.

#### Scenario: No privileges
- **WHEN** a step reads its process status
- **THEN** the effective capabilities are empty and `NoNewPrivs` is 1, and writing to `/usr` fails

### Requirement: Container mounts
The container SHALL mount, each at its resolved host path, only the instance workspace, its config directory, its `XDG_CONFIG_HOME` (read-only), a dedicated sandbox home directory and the paths of `NEARBEE_SANDBOX_MOUNTS_RO` (read-only); in the config directory the policy, origin key and server key files SHALL be read-only when they exist, while `trust.json` and `projects.json` stay writable because apb replaces them by rename. A mount path holding a comma, a quote or a newline SHALL be refused.

#### Scenario: No host home
- **WHEN** a step in the container lists the owner's home directory
- **THEN** only the mounted paths exist under it and `~/.ssh` and the repository's `.env` are absent

### Requirement: Protected config files
A start SHALL create the policy file with no settings when it is missing, so it is always bound read-only, and SHALL refuse a protected config file that is a symlink, not a regular file of the owner, or does not resolve to its own path.

#### Scenario: Planted policy link
- **WHEN** a step replaces `policy.yaml` in the config directory with a symlink to a host file and the instance is started again
- **THEN** the start refuses and names the file, and no host file is mounted

### Requirement: Egress allowlist
The container SHALL sit on an internal network with no route out, no DNS and no host address on its bridge; each host in `sandbox/egress.allow` plus `NEARBEE_SANDBOX_ALLOW` SHALL resolve, through the container's hosts file, to a gateway that passes TLS connections through unchanged to the server name the client sent, and only when that name is in the allowlist.

#### Scenario: Allowed host
- **WHEN** a step calls `https://cloud-api.near.ai/v1/models`
- **THEN** the call reaches NEAR AI Cloud and the server certificate fingerprint seen in the container equals the one seen from the host

#### Scenario: Other host
- **WHEN** a step calls `https://example.com` or a raw IP address
- **THEN** the name does not resolve or the connection fails

#### Scenario: Host services
- **WHEN** a step connects to the first address of the internal subnet (for example `172.30.77.1:22`)
- **THEN** the connection fails

#### Scenario: Spoofed server name
- **WHEN** a step connects to the gateway with a server name outside the allowlist
- **THEN** the connection is closed without reaching any upstream

### Requirement: apb CLI in the sandbox
In sandbox mode the script's `apb`, `install` and `register` subcommands and Nearbee's apb CLI calls (through `APB_BIN` set to the absolute path of `scripts/apb-sandboxed`) SHALL run the apb CLI inside the container with the caller's working directory when it is mounted and `APB_CONFIG_DIR`, bounded by a timeout inside the container.

#### Scenario: List through the wrapper
- **WHEN** `scripts/apb-sandboxed list` runs in the workspace on the host
- **THEN** it prints the playbooks of the sandboxed instance

#### Scenario: Publish install through the wrapper
- **WHEN** Nearbee installs a playbook with `APB_BIN` set to the wrapper
- **THEN** the import, the approval and the export read-back succeed

### Requirement: Shared paths on the host side
Host code SHALL never read or write a file in the workspace, the config directory or the sandbox home that a step could have swapped or turned into a symlink: install bundles SHALL reach apb on stdin with the accept digest computed from the bundle, the export read-back SHALL come from stdout, and the workspace writes of `install` and `register` SHALL run inside the container.

#### Scenario: Planted install link
- **WHEN** a step plants `.apb/config.yaml.nearbee-tmp` or `opencode.json` in the workspace as a symlink to a host file and `install` runs
- **THEN** the host file is unchanged

### Requirement: Stop and teardown
`stop` SHALL stop the dashboard container, removing it even when the client was killed, and leave the gateway; `down` SHALL also remove the gateway and the two networks; `stop` interrupts every run and the next `start` SHALL run `apb resume --all` once the dashboard answers.

#### Scenario: Down
- **WHEN** `NEARBEE_APB_SANDBOX=1 scripts/apb-instance.sh down` runs
- **THEN** no container or network created by the sandbox remains

### Requirement: API key
Sandbox `start` SHALL issue an apb server key only when the instance has none, write it as `APB_API_KEY` to the `.env` file Nearbee reads without printing it, never pass it into the container, and refuse `APB_API_KEY` in `NEARBEE_APB_ENV_VARS`.

#### Scenario: Second start
- **WHEN** the sandboxed instance starts again with a key issued
- **THEN** no new key is issued and the health check succeeds with the stored key

### Requirement: Image matches the host binaries
Sandbox `start` SHALL refuse to run when the sha256 of the image's `apb` or `nearbee` differs from the host binary it was built from.

#### Scenario: Updated host binary
- **WHEN** the host `nearbee` changed after the image was built
- **THEN** start refuses and names the rebuild command

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

### Requirement: Build and audit toolchains in the image
The sandbox image SHALL contain, each at a pinned version, a C toolchain, `bubblewrap`, Go, Rust, uv, `osv-scanner` and `pip-audit`, and the egress allowlist SHALL include the npm, Yarn, PyPI, Go module and crates registries and the OSV database hosts, so a script step can install, build, test and scan a buyer's repository.

#### Scenario: Tools present
- **WHEN** the image is built from `sandbox/Dockerfile`
- **THEN** `go version`, `cargo --version`, `uv --version`, `osv-scanner --version`, `pip-audit --version` and `cc --version` inside it print the pinned versions

#### Scenario: Registries reachable, others not
- **WHEN** a step in the sandbox opens `registry.npmjs.org` and an unlisted host
- **THEN** the first connects and the second has no route

### Requirement: Toolchains stay pinned
Go and Rust SHALL stay on the image's toolchains, so a repository's `toolchain` line or `rust-toolchain` file SHALL NOT start a download, and `pip-audit` and its dependencies SHALL be installed from a lock that pins each by SHA-256.

#### Scenario: A repository pins another Rust
- **WHEN** a step runs `cargo --version` in a repository whose `rust-toolchain.toml` names `channel = "1.88"`
- **THEN** cargo answers with the image's pinned toolchain and nothing is downloaded

#### Scenario: pip-audit from its lock
- **WHEN** the image is built
- **THEN** `pip-audit` and every dependency are installed with `--require-hashes` from `sandbox/pip-audit.lock`
