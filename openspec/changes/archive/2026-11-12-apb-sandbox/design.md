# Design: apb sandbox

## 1. Image (`sandbox/`)

1.1. `Dockerfile`: `FROM ubuntu:26.04` (the host release, so copied
binaries find the same glibc). apt: `ca-certificates git gh curl jq
ripgrep python3 python3-venv tini poppler-utils imagemagick util-linux
nodejs npm`. `npm install -g` pinned `@fission-ai/openspec@2.0.0` and
`@itechmeat/open-code-review@1.13.8-fork.1`; a venv under `/opt/code-ranker`
with `code-ranker==5.0.4`, linked into `/usr/local/bin`. `COPY bin/
/usr/local/bin/`. Env `DISABLE_AUTOUPDATER=1` and
`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` (no `OPENCODE_` variable: the
tier check treats them as route changes). Entry point `tini --`.

1.2. `build.sh [tag]`: copies `apb` (`APB_BIN` or PATH), `nearbee`
(`NEARBEE_BIN` or PATH) and, when present, the resolved binaries of
`claude`, `codex` and `opencode` (`NEARBEE_SANDBOX_CLIS` overrides the
list) into `sandbox/bin/` (git-ignored), builds `nearbee-sandbox:<tag>`
(default `local`) and removes `bin/`. A CLI that is a script or missing is
skipped with a line saying so.

1.3. `egress.allow`: one host per line, `#` comments. Default:
`cloud-api.near.ai`, `github.com`, `api.github.com`, `codeload.github.com`,
`objects.githubusercontent.com`, `api.anthropic.com`, `api.openai.com`.

## 2. Instance (`scripts/apb-instance.sh`, `NEARBEE_APB_SANDBOX=1`)

2.1. Names: networks `nearbee-sbx-int` (internal, subnet from
`NEARBEE_SANDBOX_SUBNET`, default `172.30.77.0/24`) and `nearbee-sbx-out`;
gateway container `nearbee-apb-gw` (image `nginx:1.29-alpine`) at `.2`,
dashboard container `nearbee-apb-sbx` at the fixed address `.3`.

2.2. Gateway config is generated from the allowlist: an nginx `stream`
block with `ssl_preread on` on 443, a `map $ssl_preread_server_name` with
one entry per allowed host to `<host>:443` and `default` to a closed local
port, `resolver 127.0.0.11 ipv6=off valid=30s` (Docker's resolver on the
outside network), and a second `server` that forwards `7322` to the
dashboard's fixed IP. The gateway runs with `--restart unless-stopped`,
publishes `127.0.0.1:$PORT:7322` only, carries a label with the sha256 of
its config, and is recreated on `start` when the hash differs (started when
it exists but is stopped).

2.3. API key: apb refuses a non-loopback bind without a server key. On
`start` the script runs `apb server key list` (one-off container, 2.7);
with no key it issues one and writes `APB_API_KEY=<key>` to the `.env`
Nearbee reads (`NEARBEE_ENV_FILE`; the docs say it must be the file next to
`nearbee.yaml`); with a key present but no `APB_API_KEY` in that file it
stops with a message. The key is never printed, never passed into the
container, and `refuse_opencode_vars` also refuses `APB_API_KEY` in
`NEARBEE_APB_ENV_VARS`. The health check reads the header from a 0600 file
(`curl -H @file`), never the command line.

2.4. Dashboard: `systemd-run --user --unit nearbee-apb -p
ExecStopPost=docker rm -f nearbee-apb-sbx`, after `docker rm -f
nearbee-apb-sbx`, runs `docker run --rm --name nearbee-apb-sbx --network
nearbee-sbx-int --ip <.3> --dns 127.0.0.1 --add-host <host>:<gateway ip>...
--user <uid>:<gid> --read-only --tmpfs /tmp:rw,exec,nosuid,nodev,size=1g
--cap-drop ALL --security-opt no-new-privileges --pids-limit 1024 --memory
4g --cpus 4 <mounts> -e VAR... nearbee-sandbox:<tag> apb dashboard
--no-open --bind 0.0.0.0 --port 7322`.
The unit's environment: the `EnvironmentFile` of the listed variables plus
`APB_CONFIG_DIR`, `XDG_CONFIG_HOME`, `HOME` (the sandbox home,
`~/.local/share/nearbee/sandbox-home`), `NEARBEE_BIN=/usr/local/bin/nearbee`,
a container `PATH`, and `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`,
`GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL` (the nearbee identity the
script already uses); each is passed with `-e NAME` so the container gets
the unit's value. `DOCKER_CONFIG` is set to `~/.local/share/nearbee/docker-config`
(owned by Nearbee, outside every mount) and never passed in.
After health answers 200, `start` runs `apb resume --all` in the workspace
inside the container, since a sandboxed `stop` interrupts every run.

2.5. Mounts, each at its `realpath` and created as the owner before
`docker run` (a missing read-only path is refused; `start` also refuses
when `nearbee.yaml`'s `apb.workspace_path` is not the mounted workspace,
when that config is found):
1. the workspace, read-write;
2. the config directory, read-write, with `policy.yaml`, `run-origin.key`
   and `server-auth.yaml` bound read-only over it when they exist (start
   creates `policy.yaml` with no settings when it is missing and refuses
   any of the three that is a symlink, not a regular file of the owner,
   or does not resolve to its own path; code review, finding 1);
   `trust.json` and `projects.json` stay writable: apb replaces them by
   rename (measured), which a bound file cannot take, so a step can change
   approvals and the project registry (documented limit);
3. `XDG_CONFIG_HOME`, read-only;
4. the sandbox home, read-write, with an empty read-only directory bound
   over `$SANDBOX_HOME/.opencode` (start refuses when the host side holds
   entries);
5. each path of `NEARBEE_SANDBOX_MOUNTS_RO`, read-only.

2.6. `apb` subcommand, `install` and `register` and `scripts/apb-sandboxed`
run `docker exec -i -u <uid>:<gid> -w <dir> -e APB_CONFIG_DIR
nearbee-apb-sbx timeout --kill-after=5 <N> apb ...`, where `<dir>` is
`$PWD` when it is under a mount and the workspace otherwise, and `N` is 25 s
by default (`NEARBEE_SANDBOX_CLI_TIMEOUT`). The bundles of the script are
built in a private host directory under `$XDG_RUNTIME_DIR` and reach apb on
stdin (`apb import /dev/stdin`), with the accept digest computed by the
script; the profiles, `opencode.json` and the decisions edit are written by
`scripts/apb-workspace-write.py` inside the container, and `register` makes
the directory and runs git there too (code review, findings 2 and 3: steps
write the workspace and the config directory under the owner's uid, so host
code must not follow a link or reuse a path there). `apb-sandboxed` must be named by an absolute
path in `APB_BIN` (Nearbee spawns it from the workspace). When the dashboard
container is down, `register` and `install` use a one-off `docker run --rm`
with the same mounts (2.7).

2.7. One-off container: the same image, mounts, user and limits on
`--network none`, for `register`, `install`, `server key list/issue` and
`apb` while the dashboard is down.

2.8. `stop`: stops the unit (the container goes with it and with
`ExecStopPost`). `down`: `stop`, then removes the gateway and both
networks. `status` adds the container and gateway state and compares the
image labels of 2.9 with the host binaries.

2.9. Stale binaries: `build.sh` labels the image with the sha256 of each
copied `apb` and `nearbee`; `start` refuses when the host's binary differs
(rebuild the image).

## 2a. Nearbee change (Rust)

`crates/nearbee-apb/src/install.rs`: the publish install writes no
temporary file. Every import call passes the bundle on stdin (`apb import
/dev/stdin`, which apb 1.0.0-beta.7 reads like any file and digests over
the raw bytes) and the read-back takes `apb export <id>` from stdout, so a
containerised CLI needs no shared file and a sandboxed step cannot swap
one. (The first version put the files under the config directory; the code
review showed that a step can write there.) The behaviour in the default
mode is unchanged.

## 3. Limits stated in the docs

One container per instance, not per hire: hires share it and are kept
apart by worktrees only; a step can leave files in the shared sandbox home
and the writable parts of the config directory that later hires see. A
sandboxed `stop` interrupts every run (`start` resumes them). Containers share the host kernel. The allowlist is
by TLS server name; an allowed host that accepts uploads (GitHub with a
token) remains a channel, and a host behind a shared CDN allows domain
fronting. OpenCode needs `registry.npmjs.org` and `models.dev` at run time
unless its cache is warmed in the sandbox home; the docs say which. Agent CLI logins go in as API keys or tokens
listed in `NEARBEE_APB_ENV_VARS`, never the owner's config directories.

## 4. Verification

Shell checks run by the implementer, then the integrator's live check: the
sandboxed start, the health call, the wrapper through `nearbee` itself
(`nearbee list --dry-run` if it exists, else the publish install test
against the sandbox), the egress scenarios, one fast audit run through the
REST API on a small public repository (about USD 0.002), and the
subscription smoke (`live_smoke_own`) with a Claude token in the sandbox.
The default mode is checked unchanged.
