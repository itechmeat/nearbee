# Spec Delta

## ADDED Requirements

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
