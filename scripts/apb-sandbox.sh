# Sandbox mode of scripts/apb-instance.sh (NEARBEE_APB_SANDBOX=1), sourced
# by it; not run on its own. The instance's dashboard runs inside the image
# nearbee-sandbox (sandbox/build.sh) on an internal Docker network with no
# route out and no DNS. Its hosts file maps every allowed host to a gateway
# that passes TLS through unchanged by server name (sandbox/egress.allow
# plus NEARBEE_SANDBOX_ALLOW) and publishes the dashboard on
# 127.0.0.1:$PORT. Settings (environment):
#   NEARBEE_SANDBOX_TAG       image tag (local)
#   NEARBEE_SANDBOX_SUBNET    internal network, a /24 (172.30.77.0/24); the
#                             gateway is .2, the dashboard .3
#   NEARBEE_SANDBOX_ALLOW     more allowed hosts, space separated
#   NEARBEE_SANDBOX_MOUNTS_RO more host paths mounted read-only, space
#                             separated (no comma, quote or newline in one)
#   NEARBEE_SANDBOX_HOME      the container's HOME (~/.local/share/nearbee/sandbox-home)
#   NEARBEE_SANDBOX_CLI_TIMEOUT  seconds one apb CLI call may take (25)
#   NEARBEE_APB_WORKSPACE     the workspace mounted read-write
#                             (~/.local/share/nearbee/apb-workspace)
# shellcheck shell=bash

NB_DATA="$HOME/.local/share/nearbee"
SBX_TAG="${NEARBEE_SANDBOX_TAG:-local}"
SBX_IMAGE="nearbee-sandbox:$SBX_TAG"
GW_IMAGE="nginx:1.29-alpine"
SBX_NAME="$UNIT-sbx"
GW_NAME="$UNIT-gw"
NET_BASE="${UNIT/apb/sbx}"
NET_INT="$NET_BASE-int"
NET_OUT="$NET_BASE-out"
SUBNET="${NEARBEE_SANDBOX_SUBNET:-172.30.77.0/24}"
SUBNET_PREFIX="${SUBNET%.*}"
GW_IP="$SUBNET_PREFIX.2"
SBX_IP="$SUBNET_PREFIX.3"
SBX_HOME="${NEARBEE_SANDBOX_HOME:-$NB_DATA/sandbox-home}"
SBX_EMPTY="$NB_DATA/sandbox-empty"
GW_DIR="$NB_DATA/$UNIT-gateway"
SBX_DOCKER_CONFIG="$NB_DATA/docker-config"
WORKSPACE="${NEARBEE_APB_WORKSPACE:-$NB_DATA/apb-workspace}"
CLI_TIMEOUT="${NEARBEE_SANDBOX_CLI_TIMEOUT:-25}"
CONTAINER_PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
GIT_NAME=nearbee
GIT_EMAIL=nearbee@nearbee.local
# Files of the config directory bound read-only over the writable mount
# (when they exist at start). apb rewrites trust.json and projects.json by
# rename during normal work (an approval, every project touch), so those two
# stay writable; see docs/apb.md, "Sandboxed instance".
RO_CONFIG_FILES="policy.yaml server-auth.yaml run-origin.key"
# More read-only mounts, an array (split on whitespace, never globbed).
read -ra SBX_MOUNTS_RO <<< "${NEARBEE_SANDBOX_MOUNTS_RO:-}"
# The gateway's limits: it parses the ClientHello of the untrusted
# container, so it runs as the image's nginx user (101) with no capability
# at all (Docker lets a container bind ports below 1024 unprivileged), no
# new privileges and a read-only root.
GW_LIMITS=(
  --user 101:101 --cap-drop ALL --security-opt no-new-privileges
  --read-only --tmpfs /tmp:rw,nosuid,nodev,size=16m,uid=101,gid=101
  --tmpfs /var/cache/nginx:rw,nosuid,nodev,size=16m,uid=101,gid=101
  --pids-limit 64 --memory 256m
)

docker_cli() { DOCKER_CONFIG="$SBX_DOCKER_CONFIG" docker "$@"; }

sbx_need_docker() {
  command -v docker >/dev/null || die "docker is not installed; the sandbox needs it"
  mkdir -p "$SBX_DOCKER_CONFIG"
  chmod 700 "$SBX_DOCKER_CONFIG"
  docker_cli info >/dev/null 2>&1 || die "the Docker daemon does not answer"
}

# The resolved path of $1 (which must exist).
real() { realpath -e -- "$1" 2>/dev/null || die "$1 does not exist"; }

# A path pasted into `docker --mount type=bind,src=...`: a comma, a quote or
# a newline would add mount options of its own, so none is allowed.
mount_src() {
  case "$1" in
    *,*|*\"*|*$'\n'*) die "the path $1 holds a comma, a quote or a newline; it cannot be passed to docker --mount" ;;
  esac
  printf '%s' "$1"
}

# The protected config files are bound read-only into the dashboard, so a
# step must never choose what is bound: none may be a symlink (Docker would
# bind its host target), an existing one must be a regular file of the owner
# at exactly its own path, and policy.yaml is created with apb's defaults
# when missing, so it is always bound and a step can never write the one a
# later start protects. Steps write the config directory under the same
# uid, so this runs only while no sandbox container is up (start removes it
# first).
sbx_check_protected() {
  local cfg f p
  cfg="$(real "$CONFIG_DIR")"
  for f in $RO_CONFIG_FILES; do
    p="$cfg/$f"
    if [ -L "$p" ]; then
      die "$p is a symlink; a protected config file must be a regular file (remove the link)"
    fi
    [ -e "$p" ] || continue
    [ -f "$p" ] || die "$p is not a regular file"
    [ "$(stat -c %u -- "$p")" = "$(id -u)" ] || die "$p is not owned by uid $(id -u)"
    [ "$(realpath -e -- "$p")" = "$p" ] || die "$p does not resolve to itself"
  done
  if [ ! -e "$cfg/policy.yaml" ]; then
    write_policy "$cfg/policy.yaml" <<'YAML'
# nearbee: default (scripts/apb-instance.sh). Sets nothing, so apb's own
# defaults apply; written so the sandboxed dashboard always binds this file
# read-only. allow-billed replaces it.
{}
YAML
  fi
}

# Mount sources, created as the owner so Docker never creates them as root.
sbx_prepare_dirs() {
  mkdir -p "$CONFIG_DIR" "$XDG_DIR" "$SBX_HOME" "$SBX_EMPTY" "$WORKSPACE"
  chmod 700 "$CONFIG_DIR" "$XDG_DIR" "$SBX_HOME"
  chmod 555 "$SBX_EMPTY"
  [ -z "$(ls -A "$SBX_EMPTY")" ] || die "$SBX_EMPTY must be empty"
  # OpenCode reads ~/.opencode; an empty read-only directory is bound over
  # it, and anything on the host side would be hidden only by that bind.
  mkdir -p "$SBX_HOME/.opencode"
  [ -z "$(ls -A "$SBX_HOME/.opencode")" ] \
    || die "$SBX_HOME/.opencode holds entries; the sandbox home must hold no OpenCode configuration there"
  local p
  for p in "${SBX_MOUNTS_RO[@]}"; do
    [ -e "$p" ] || die "NEARBEE_SANDBOX_MOUNTS_RO names $p, which does not exist"
    mount_src "$(real "$p")" >/dev/null
  done
  sbx_check_protected
}

# docker run arguments for the mounts. $1: "dashboard" binds the protected
# config files read-only; "admin" leaves them writable (a key issue).
sbx_mount_args() {
  local mode="$1" ws cfg xdg home empty f p
  ws="$(mount_src "$(real "$WORKSPACE")")"; cfg="$(mount_src "$(real "$CONFIG_DIR")")"
  xdg="$(mount_src "$(real "$XDG_DIR")")"; home="$(mount_src "$(real "$SBX_HOME")")"
  empty="$(mount_src "$(real "$SBX_EMPTY")")"
  MOUNT_ARGS=(
    --mount "type=bind,src=$ws,dst=$ws"
    --mount "type=bind,src=$cfg,dst=$cfg"
  )
  if [ "$mode" = dashboard ]; then
    for f in $RO_CONFIG_FILES; do
      # Never a symlink (sbx_check_protected refused one at start).
      if [ -L "$cfg/$f" ]; then
        die "$cfg/$f is a symlink; a protected config file must be a regular file"
      fi
      if [ -f "$cfg/$f" ]; then
        MOUNT_ARGS+=(--mount "type=bind,src=$cfg/$f,dst=$cfg/$f,readonly")
      fi
    done
  fi
  MOUNT_ARGS+=(
    --mount "type=bind,src=$xdg,dst=$xdg,readonly"
    --mount "type=bind,src=$home,dst=$home"
    --mount "type=bind,src=$empty,dst=$home/.opencode,readonly"
  )
  MOUNTED_PATHS=("$ws" "$cfg" "$home" "$xdg")
  for p in "${SBX_MOUNTS_RO[@]}"; do
    p="$(mount_src "$(real "$p")")"
    MOUNT_ARGS+=(--mount "type=bind,src=$p,dst=$p,readonly")
    MOUNTED_PATHS+=("$p")
  done
}

# The limits every sandbox container runs with.
SBX_LIMITS=(
  --user "$(id -u):$(id -g)"
  --read-only --tmpfs /tmp:rw,exec,nosuid,nodev,size=1g
  --cap-drop ALL --security-opt no-new-privileges
  --pids-limit 1024 --memory 4g --cpus 4
)

# The environment the container gets, as NAME=value pairs: the same values
# the unit carries (spec apb-sandbox, Same environment).
sbx_env_pairs() {
  SBX_ENV=(
    "APB_CONFIG_DIR=$(real "$CONFIG_DIR")"
    "XDG_CONFIG_HOME=$(real "$XDG_DIR")"
    "HOME=$(real "$SBX_HOME")"
    "NEARBEE_BIN=/usr/local/bin/nearbee"
    "PATH=$CONTAINER_PATH"
    "GIT_AUTHOR_NAME=$GIT_NAME" "GIT_AUTHOR_EMAIL=$GIT_EMAIL"
    "GIT_COMMITTER_NAME=$GIT_NAME" "GIT_COMMITTER_EMAIL=$GIT_EMAIL"
  )
}

# The working directory inside the container: $1 when it lies under a
# mounted path, else the workspace.
sbx_workdir() {
  local dir m
  dir="$(realpath -e -- "$1" 2>/dev/null || true)"
  if [ -n "$dir" ]; then
    for m in "${MOUNTED_PATHS[@]}"; do
      case "$dir/" in "$m"/*) printf '%s' "$dir"; return ;; esac
    done
  fi
  real "$WORKSPACE"
}

# Register and install work only in a directory the container mounts.
sbx_require_mounted() {
  sbx_mount_args dashboard
  [ "$(sbx_workdir "$1")" = "$(real "$1")" ] \
    || die "$1 is not inside a path the sandbox mounts (the workspace is $WORKSPACE; set NEARBEE_APB_WORKSPACE)"
}

sbx_running() { [ "$(docker_cli inspect -f '{{.State.Running}}' "$SBX_NAME" 2>/dev/null || true)" = true ]; }

# Runs a command in the sandbox from directory $1: inside the running
# dashboard container (same processes, same egress) or, while it is down, a
# one-off container on no network. $2: dashboard | admin (see
# sbx_mount_args; admin always takes a one-off container). The call is
# bounded by a timeout inside the container, so a killed client leaves no
# process behind for long. Stdin is passed through.
sbx_exec() {
  local dir="$1" mode="$2" wd cfg
  shift 2
  sbx_mount_args "$mode"
  wd="$(sbx_workdir "$dir")"
  cfg="$(real "$CONFIG_DIR")"
  if [ "$mode" != admin ] && sbx_running; then
    local mounted
    mounted="$(docker_cli inspect -f '{{index .Config.Labels "nearbee.config-dir"}}' "$SBX_NAME")"
    [ "$mounted" = "$cfg" ] || die "the running sandbox serves $mounted, not $cfg"
    docker_cli exec -i -u "$(id -u):$(id -g)" -w "$wd" -e "APB_CONFIG_DIR=$cfg" "$SBX_NAME" \
      timeout --kill-after=5 "$CLI_TIMEOUT" "$@"
    return
  fi
  sbx_env_pairs
  local envs=() e
  for e in "${SBX_ENV[@]}"; do envs+=(-e "$e"); done
  docker_cli image inspect "$SBX_IMAGE" >/dev/null 2>&1 \
    || die "no image $SBX_IMAGE; build it: sandbox/build.sh $SBX_TAG"
  docker_cli run --rm -i --network none "${SBX_LIMITS[@]}" "${MOUNT_ARGS[@]}" "${envs[@]}" \
    -w "$wd" "$SBX_IMAGE" timeout --kill-after=5 "$CLI_TIMEOUT" "$@"
}

# The apb CLI in the sandbox (sbx_exec).
sbx_apb() {
  local dir="$1" mode="$2"
  shift 2
  sbx_exec "$dir" "$mode" apb "$@"
}

# `register` in sandbox mode: the directory must lie under a mounted path
# (checked on its resolved path, which the container sees the same way);
# it is made, and git runs in it, inside the container.
sbx_register() {
  local dir="$1" git="$2" target m ok=""
  sbx_mount_args dashboard
  target="$(realpath -m -- "$dir")"
  for m in "${MOUNTED_PATHS[@]}"; do
    case "$target/" in "$m"/*) ok=1 ;; esac
  done
  [ -n "$ok" ] || die "$dir is not inside a path the sandbox mounts (the workspace is $WORKSPACE; set NEARBEE_APB_WORKSPACE)"
  # shellcheck disable=SC2016 # expanded by the shell in the container
  sbx_exec "$WORKSPACE" dashboard sh -c '
    mkdir -p -- "$1" || exit 1
    if [ "$2" = --git ] && [ ! -d "$1/.git" ]; then
      git -C "$1" init -q &&
        git -C "$1" -c user.name=nearbee -c user.email=nearbee@nearbee.local commit -q --allow-empty -m init
    fi' sh "$target" "$git" || die "cannot make $dir in the sandbox"
  sbx_require_mounted "$target"
}

# The image must carry the host's apb and nearbee (spec apb-sandbox, Image
# matches the host binaries).
sbx_check_image() {
  docker_cli image inspect "$SBX_IMAGE" >/dev/null 2>&1 \
    || die "no image $SBX_IMAGE; build it: sandbox/build.sh $SBX_TAG"
  local name host want have
  for name in apb nearbee; do
    if [ "$name" = apb ]; then host="$APB_BIN"; else host="$(nearbee_bin)"; fi
    [ -n "$host" ] && [ -f "$host" ] || host="$(command -v "$name" || true)"
    [ -n "$host" ] || die "$name is not on PATH (set ${name^^}_BIN); the image cannot be checked against it"
    want="$(sha256sum "$(readlink -f "$host")" | cut -d' ' -f1)"
    have="$(docker_cli image inspect -f "{{index .Config.Labels \"nearbee.sandbox.$name-sha256\"}}" "$SBX_IMAGE")"
    [ "$want" = "$have" ] \
      || die "the $name in $SBX_IMAGE differs from $host; rebuild the image: sandbox/build.sh $SBX_TAG"
  done
}

# nearbee.yaml next to the .env file, when found, must name the mounted
# workspace (spec: the runs happen where the container can see them).
sbx_check_workspace() {
  local dir cfgfile ws
  dir="$(dirname "$ENV_FILE")"
  for cfgfile in "$dir/nearbee.yaml" "$dir/nearbee.local.yaml"; do
    [ -f "$cfgfile" ] || continue
    ws="$(python3 - "$cfgfile" <<'PY'
import os, re, sys
path = sys.argv[1]
inside = False
for line in open(path, encoding="utf-8"):
    if re.match(r"^apb\s*:", line):
        inside = True
        continue
    if inside and line.strip() and not line.startswith((" ", "\t")):
        break
    m = re.match(r"^\s+workspace_path\s*:\s*(.+?)\s*$", line) if inside else None
    if m:
        v = m.group(1).split(" #")[0].strip().strip("'\"")
        v = os.path.expanduser(v)
        if not os.path.isabs(v):
            v = os.path.join(os.path.dirname(os.path.abspath(path)), v)
        print(v)
        break
PY
)"
    [ -n "$ws" ] || continue
    [ "$(realpath -m -- "$ws")" = "$(real "$WORKSPACE")" ] \
      || die "$cfgfile names the workspace $ws, but the sandbox mounts $WORKSPACE (set NEARBEE_APB_WORKSPACE)"
  done
}

# APB_API_KEY in Nearbee's .env, written by the key issue below.
env_has_api_key() {
  [ -f "$ENV_FILE" ] || return 1
  local probe rc=0
  probe="$(umask 077; mktemp "$RUNTIME_DIR/nearbee-apb-probe.XXXXXX")"
  python3 "$REPO_ROOT/scripts/dotenv-export.py" "$ENV_FILE" "$probe" APB_API_KEY >/dev/null 2>&1 || rc=$?
  if [ "$rc" -ne 0 ]; then rm -f "$probe"; die "$ENV_FILE cannot be parsed"; fi
  if [ -s "$probe" ]; then rm -f "$probe"; return 0; fi
  rm -f "$probe"; return 1
}

# The dashboard binds a non-loopback address inside the container, which
# apb allows only with a server key. One is issued only when the instance
# has none; it goes to Nearbee's .env and is never printed or passed in.
sbx_api_key() {
  local listing
  listing="$(sbx_apb "$WORKSPACE" admin server key list 2>&1)" || die "apb server key list failed"
  if printf '%s\n' "$listing" | grep -q '^no server keys'; then
    local issued key
    issued="$(sbx_apb "$WORKSPACE" admin server key issue 2>/dev/null)" || die "apb server key issue failed"
    key="$(printf '%s\n' "$issued" | sed -n '/^[[:space:]]*$/d; p' | head -n1 | tr -d '[:space:]')"
    issued=""
    case "$key" in apb_*) ;; *) die "apb server key issue printed no key" ;; esac
    APB_KEY_VALUE="$key" python3 - "$ENV_FILE" <<'PY'
import os, re, sys
path = sys.argv[1]
key = os.environ["APB_KEY_VALUE"]
lines = open(path, encoding="utf-8").read().splitlines() if os.path.exists(path) else []
lines = [l for l in lines if not re.match(r"^\s*(export\s+)?APB_API_KEY\s*=", l)]
lines.append(f"APB_API_KEY={key}")
tmp = path + ".nearbee-tmp"
fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w", encoding="utf-8") as f:
    f.write("\n".join(lines) + "\n")
if os.path.exists(path):
    os.chmod(tmp, os.stat(path).st_mode & 0o777 & ~0o077)
os.replace(tmp, path)
PY
    key=""
    echo "apb-instance: issued a server key for the sandboxed dashboard; APB_API_KEY written to $ENV_FILE"
  elif ! env_has_api_key; then
    die "the instance has a server key but $ENV_FILE has no APB_API_KEY; add it, or revoke the key (scripts/apb-instance.sh apb server key revoke <id> with the instance stopped)"
  fi
}

# A 0600 file with the Authorization header from Nearbee's .env, for
# `curl -H @file`: the key never appears on a command line.
HDR_FILE=""
sbx_header_file() {
  HDR_FILE="$(umask 077; mktemp "$RUNTIME_DIR/nearbee-apb-hdr.XXXXXX")"
  python3 - "$REPO_ROOT/scripts/dotenv-export.py" "$ENV_FILE" "$HDR_FILE" <<'PY' || { rm -f "$HDR_FILE"; HDR_FILE=""; }
import importlib.util, sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dotenv_export", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
values = mod.parse(open(sys.argv[2], encoding="utf-8", newline="").read())
key = values.get("APB_API_KEY", "")
if not key:
    sys.exit(1)
with open(sys.argv[3], "w", encoding="utf-8") as f:
    f.write(f"Authorization: Bearer {key}\n")
PY
}
drop_header_file() { if [ -n "$HDR_FILE" ]; then rm -f "$HDR_FILE"; HDR_FILE=""; fi; }

sbx_health() {
  if [ -n "$HDR_FILE" ]; then
    curl -s -o /dev/null -w '%{http_code}' --max-time 3 -H "@$HDR_FILE" "http://127.0.0.1:$PORT/api/health" || true
  else
    health
  fi
}

# The allowlist: sandbox/egress.allow plus NEARBEE_SANDBOX_ALLOW.
sbx_allowed_hosts_raw() {
  local h extra=()
  read -ra extra <<< "${NEARBEE_SANDBOX_ALLOW:-}"
  {
    sed -e 's/#.*//' -e 's/[[:space:]]//g' "$REPO_ROOT/sandbox/egress.allow"
    for h in "${extra[@]}"; do printf '%s\n' "$h"; done
  } | sed '/^$/d' | tr 'A-Z' 'a-z' | sort -u
}
sbx_allowed_hosts() {
  local h
  while read -r h; do
    [[ "$h" =~ ^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$ ]] && printf '%s\n' "$h"
  done < <(sbx_allowed_hosts_raw)
}

# nginx stream config: TLS passed through by server name to allowed hosts
# only (anything else goes to a closed port), and the dashboard port.
sbx_gateway_conf() {
  local h
  cat <<EOF
# Generated by scripts/apb-instance.sh (sandbox mode); do not edit.
worker_processes 1;
error_log /dev/stderr warn;
pid /tmp/nginx.pid;
events { worker_connections 1024; }
stream {
  resolver 127.0.0.11 ipv6=off valid=30s;
  map \$ssl_preread_server_name \$nearbee_upstream {
    hostnames;
    default 127.0.0.1:9;
EOF
  while read -r h; do printf '    %s %s:443;\n' "$h" "$h"; done < <(sbx_allowed_hosts)
  cat <<EOF
  }
  server {
    listen $GW_IP:443;
    ssl_preread on;
    proxy_connect_timeout 10s;
    proxy_pass \$nearbee_upstream;
  }
  server {
    listen 7322;
    proxy_pass $SBX_IP:7322;
  }
}
EOF
}

# The internal network: no route out, and no host address on its bridge
# (gateway mode isolated), so the container cannot reach host services at
# the bridge address either. One made earlier with another subnet or with
# a host address is made again (the dashboard container is down by then).
sbx_networks() {
  local have
  have="$(docker_cli network inspect -f '{{range .IPAM.Config}}{{.Subnet}}{{end}}|{{index .Options "com.docker.network.bridge.gateway_mode_ipv4"}}' "$NET_INT" 2>/dev/null || true)"
  if [ -n "$have" ] && [ "$have" != "$SUBNET|isolated" ]; then
    docker_cli rm -f "$GW_NAME" >/dev/null 2>&1 || true
    docker_cli network rm "$NET_INT" >/dev/null \
      || die "the network $NET_INT ($have) must be made again and cannot be removed; run: scripts/apb-instance.sh down"
    have=""
  fi
  [ -n "$have" ] \
    || docker_cli network create --internal -o com.docker.network.bridge.gateway_mode_ipv4=isolated \
      --subnet "$SUBNET" "$NET_INT" >/dev/null \
    || die "cannot create the network $NET_INT ($SUBNET)"
  docker_cli network inspect "$NET_OUT" >/dev/null 2>&1 \
    || docker_cli network create "$NET_OUT" >/dev/null \
    || die "cannot create the network $NET_OUT"
}

sbx_gateway() {
  mkdir -p "$GW_DIR"
  local conf="$GW_DIR/nginx.conf" hash current state
  sbx_gateway_conf > "$conf.tmp"
  mv -f "$conf.tmp" "$conf"
  hash="$(printf '%s|%s|%s|%s|%s' "$(sha256sum "$conf" | cut -d' ' -f1)" "$PORT" "$GW_IMAGE" \
    "${GW_LIMITS[*]}" "$GW_IP" | sha256sum | cut -d' ' -f1)"
  current="$(docker_cli inspect -f '{{index .Config.Labels "nearbee.gateway-hash"}}' "$GW_NAME" 2>/dev/null || true)"
  # Reused only when it is the same gateway and attached at its address
  # (a start that failed between create and attach leaves one that is not).
  if [ -n "$current" ] && { [ "$current" != "$hash" ] \
    || [ "$(docker_cli inspect -f "{{with index .NetworkSettings.Networks \"$NET_INT\"}}{{.IPAddress}}{{end}}" "$GW_NAME" 2>/dev/null || true)" != "$GW_IP" ]; }; then
    docker_cli rm -f "$GW_NAME" >/dev/null
    current=""
  fi
  if [ -z "$current" ]; then
    docker_cli create --name "$GW_NAME" --restart unless-stopped "${GW_LIMITS[@]}" \
      --network "$NET_OUT" -p "127.0.0.1:$PORT:7322" \
      --mount "type=bind,src=$(mount_src "$(real "$conf")"),dst=/etc/nginx/nginx.conf,readonly" \
      --label "nearbee.gateway-hash=$hash" "$GW_IMAGE" >/dev/null \
      || die "cannot create the gateway $GW_NAME"
    if ! docker_cli network connect --ip "$GW_IP" "$NET_INT" "$GW_NAME"; then
      docker_cli rm -f "$GW_NAME" >/dev/null 2>&1 || true
      die "cannot attach the gateway to $NET_INT at $GW_IP"
    fi
  fi
  state="$(docker_cli inspect -f '{{.State.Running}}' "$GW_NAME")"
  [ "$state" = true ] || docker_cli start "$GW_NAME" >/dev/null || die "the gateway $GW_NAME does not start"
}

# The allowlist names only host names (checked in this shell, so a bad
# entry stops the start).
sbx_validate_allow() {
  local h
  while read -r h; do
    [[ "$h" =~ ^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$ ]] || die "not a host name in the allowlist: $h"
  done < <(sbx_allowed_hosts_raw)
}

# The passed variables may not name the server key, a value the unit and
# the container set themselves (systemd's EnvironmentFile= would win over
# the unit's own value, which the tier check reads, while the container
# got the .env value), or a DOCKER_ variable (one would steer the docker
# client, and a Docker CLI config can inject proxy variables into the
# container).
sbx_refuse_env_names() {
  local name upper e
  sbx_env_pairs
  for name in "${ENV_NAMES[@]}"; do
    upper="${name^^}"
    case "$upper" in
      APB_API_KEY) die "NEARBEE_APB_ENV_VARS names $name; the server key never goes into the sandbox" ;;
      DOCKER_*) die "NEARBEE_APB_ENV_VARS names $name; no DOCKER_ variable may reach the sandbox" ;;
    esac
    for e in "${SBX_ENV[@]}"; do
      if [ "$upper" = "${e%%=*}" ]; then
        die "NEARBEE_APB_ENV_VARS names $name, which the sandbox sets itself"
      fi
    done
  done
}

# Undoes what a failed start left: the key file and the header file.
sbx_start_failed() {
  drop_header_file
  rm -f "$APB_ENV"
}

sbx_start() {
  sbx_need_docker
  sbx_validate_allow
  refuse_opencode_vars
  if have_systemd && systemctl --user is-active --quiet "$UNIT"; then
    echo "apb-instance: $UNIT already running"; exit 0
  fi
  # No step may run while the start checks the shared directories.
  docker_cli rm -f "$SBX_NAME" >/dev/null 2>&1 || true
  prepare_xdg
  sbx_prepare_dirs
  sbx_refuse_env_names
  sbx_check_image
  sbx_check_workspace
  write_env
  trap sbx_start_failed EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  sbx_api_key
  have_systemd || die "the sandbox needs a systemd user session"
  sbx_networks
  sbx_gateway
  sbx_mount_args dashboard
  sbx_env_pairs
  local setenv=() passed=() e hosts=() h docker_bin name
  docker_bin="$(command -v docker)"
  for e in "${SBX_ENV[@]}"; do
    setenv+=("--setenv=$e")
    passed+=(-e "${e%%=*}")
  done
  for name in "${ENV_NAMES[@]}"; do passed+=(-e "$name"); done
  while read -r h; do hosts+=(--add-host "$h:$GW_IP"); done < <(sbx_allowed_hosts)
  systemctl --user reset-failed "$UNIT" 2>/dev/null || true
  systemd-run --user --unit "$UNIT" --collect \
    -p "EnvironmentFile=$APB_ENV" \
    -p "ExecStopPost=$docker_bin rm -f $SBX_NAME" \
    "${setenv[@]}" "--setenv=DOCKER_CONFIG=$SBX_DOCKER_CONFIG" \
    --working-directory="$(real "$CONFIG_DIR")" \
    "$docker_bin" run --rm --name "$SBX_NAME" --network "$NET_INT" --ip "$SBX_IP" \
    --dns 127.0.0.1 "${hosts[@]}" "${SBX_LIMITS[@]}" "${MOUNT_ARGS[@]}" "${passed[@]}" \
    --label "nearbee.config-dir=$(real "$CONFIG_DIR")" \
    -w "$(real "$WORKSPACE")" \
    "$SBX_IMAGE" apb dashboard --no-open --bind 0.0.0.0 --port 7322 >/dev/null
  sbx_header_file
  [ -n "$HDR_FILE" ] || die "no APB_API_KEY in $ENV_FILE for the health check"
  local code=""
  for _ in $(seq 1 100); do
    code="$(sbx_health)"
    [ "$code" = 200 ] && break
    sleep 0.3
  done
  drop_header_file
  [ "$code" = 200 ] || die "the sandboxed dashboard did not answer on port $PORT (last: $code); see: journalctl --user -u $UNIT"
  trap - EXIT HUP INT TERM
  # A sandboxed stop interrupts every run; pick them up again.
  CLI_TIMEOUT=120 sbx_apb "$WORKSPACE" dashboard resume --all >/dev/null 2>&1 \
    || echo "apb-instance: apb resume --all reported a problem; see: scripts/apb-instance.sh apb runs" >&2
  echo "apb-instance: sandboxed, up on http://127.0.0.1:$PORT"
}

sbx_stop() {
  if have_systemd; then systemctl --user stop "$UNIT" 2>/dev/null || true; fi
  if command -v docker >/dev/null; then docker_cli rm -f "$SBX_NAME" >/dev/null 2>&1 || true; fi
  rm -f "$APB_ENV" "$RUNTIME_DIR"/nearbee-apb.env.* "$RUNTIME_DIR"/nearbee-apb-hdr.* "$RUNTIME_DIR"/nearbee-apb-probe.*
  echo "apb-instance: stopped"
}

sbx_down() {
  sbx_stop
  command -v docker >/dev/null || return 0
  docker_cli rm -f "$GW_NAME" >/dev/null 2>&1 || true
  docker_cli network rm "$NET_INT" >/dev/null 2>&1 || true
  docker_cli network rm "$NET_OUT" >/dev/null 2>&1 || true
  echo "apb-instance: gateway and networks removed"
}

sbx_status() {
  if have_systemd; then echo "unit: $(systemctl --user is-active "$UNIT" 2>/dev/null || true)"; fi
  echo "container $SBX_NAME: $(docker_cli inspect -f '{{.State.Status}}' "$SBX_NAME" 2>/dev/null || echo absent)"
  echo "gateway $GW_NAME: $(docker_cli inspect -f '{{.State.Status}}' "$GW_NAME" 2>/dev/null || echo absent)"
  if ( sbx_check_image ) 2>/dev/null; then
    echo "image $SBX_IMAGE: matches the host apb and nearbee"
  else
    echo "image $SBX_IMAGE: missing or stale (sandbox/build.sh $SBX_TAG)"
  fi
  trap drop_header_file EXIT
  sbx_header_file
  echo "health http://127.0.0.1:$PORT/api/health: $(sbx_health)"
  drop_header_file
  echo "config dir: $CONFIG_DIR"
}
