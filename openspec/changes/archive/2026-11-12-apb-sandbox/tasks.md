# Tasks

## 1. Image

- [x] 1.1 `sandbox/Dockerfile`, `sandbox/build.sh`, `sandbox/egress.allow`, `.gitignore` entry for `sandbox/bin/` (design 1). Verify: `sandbox/build.sh` builds; `docker run --rm nearbee-sandbox:local bash -c 'apb --version; nearbee --version; openspec --version; ocr --version; code-ranker --version; magick -version; pdftoppm -v; gh --version'` all answer; the copied CLIs answer `--version`.

## 2. Instance

- [x] 2.0 Rust, test first: the publish install keeps its temporary bundle and export file under the config directory (design 2a). Verify: a test asserts the paths are under the given config directory; `cargo test -p nearbee-apb` passes. Superseded by 4.1: the bundle now goes on stdin and the export comes from stdout.

- [x] 2.1 Sandbox mode of `scripts/apb-instance.sh`: networks, gateway, key, mounts, dashboard unit, resume, `stop`, `down`, `status`, binary check (design 2.1 to 2.5, 2.7 to 2.9). Verify: start, a second start issues no key, `status`, health 200 with the key, `stop` leaves no dashboard container, `down` leaves no sandbox container or network; the default mode starts and stops as before.
- [x] 2.2 `apb`, `install`, `register` through `docker exec` or the one-off container, and `scripts/apb-sandboxed` (design 2.6, 2.7). Verify: `scripts/apb-sandboxed list` prints the instance playbooks; with `APB_BIN` set to the wrapper's absolute path, the `nearbee-apb` publish install test against the sandboxed instance (or an equivalent manual import, approval and export) succeeds.
- [x] 2.3 Spec scenarios checked by hand: same environment as `systemctl show` and no redirecting variable in `docker image inspect`, no host home, allowed host with the certificate fingerprint compared to the host's, other host, spoofed server name (`openssl s_client -servername example.com` to the gateway), and which config files apb writes during a run. Record the commands and results in the report.

## 3. Docs and gate

- [x] 3.1 `docs/apb.md` (how to run the sandboxed instance), `docs/privacy.md` and `README.md` (what the sandbox enforces and its limits, design 3). Verify: the docs name the limits of design 3.
- [x] 3.2 Native gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `bash -n` on the changed scripts (shellcheck is not installed), `openspec validate apb-sandbox --strict`.

## 4. Code review fixes (round one)

- [x] 4.1 Rust, test first: the publish install passes the bundle on stdin and reads the export from stdout, writing no temporary file (design 2a, finding 3). Verify: a test asserts `import /dev/stdin`, no `export --out` and nothing new in the config directory; `cargo test -p nearbee-apb` passes.
- [x] 4.2 Shell, test first: protected config files never symlinks and `policy.yaml` always bound (finding 1); `install` and `register` write the workspace inside the container, bundles on stdin with a locally computed digest (findings 2 and 3); internal network with gateway mode `isolated` (finding 4); no half-made gateway and no key file after a failed start (finding 5); passed names that collide with the sandbox environment or `DOCKER_` refused (finding 6); lists not globbed, mount paths without comma, quote or newline (finding 7); gateway unprivileged with no capability and a read-only root (finding 8). Verify: `cargo test -p nearbee --test apb_instance_script` passes; on the live instance, every host port tried at the bridge address `.1` is closed and the allowed host, other host and spoofed name checks of 2.3 hold.
