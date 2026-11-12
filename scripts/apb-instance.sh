#!/usr/bin/env bash
# Nearbee's own apb instance: a dashboard on its own port with its own config
# directory, so the operator's everyday apb (port 7321, ~/.config/apb), its
# profiles and its learning data are never touched.
#
#   scripts/apb-instance.sh start      start the dashboard (transient systemd --user unit)
#   scripts/apb-instance.sh stop       stop it
#   scripts/apb-instance.sh status     unit state and the HTTP answer of /api/health
#   scripts/apb-instance.sh register <dir> [--git]
#                                      make <dir> an apb project of this instance only
#   scripts/apb-instance.sh install [<workspace>]
#                                      install playbooks/profiles and playbooks/smoke into the
#                                      workspace (default ~/.local/share/nearbee/apb-workspace):
#                                      profiles as project profiles, playbooks imported as bundles
#                                      with their scripts and approved by their trust digest;
#                                      writes decisions: { enabled: false } into the project's
#                                      .apb/config.yaml
#   scripts/apb-instance.sh allow-billed
#                                      let this instance start billed and unknown-basis agent
#                                      work (NEAR AI Cloud through OpenCode) without a per-run
#                                      confirmation: writes billing: allow to its policy.yaml.
#                                      Only on the owner's go-ahead for Cloud spend (O-3, O-6);
#                                      Nearbee's runner never confirms a launch gate itself
#   scripts/apb-instance.sh apb <args> run the apb CLI against this instance's config
#   scripts/apb-instance.sh down       sandbox mode only: stop, then remove the gateway
#                                      container and the two sandbox networks
#
# Sandbox mode (NEARBEE_APB_SANDBOX=1): the dashboard runs inside the image
# nearbee-sandbox (sandbox/build.sh) behind an outbound allowlist; start,
# stop, status, register, install and apb work the same way, and
# scripts/apb-sandboxed is the APB_BIN for Nearbee's own apb CLI calls.
# Settings and limits: scripts/apb-sandbox.sh and docs/apb.md, "Sandboxed
# instance". NEARBEE_APB_UNIT (nearbee-apb) names the unit and, in sandbox
# mode, its containers and networks; the tier check reads the unit
# nearbee-apb, so change it only for a second instance under test.
#
# Settings (environment): NEARBEE_APB_PORT (7322), NEARBEE_APB_CONFIG_DIR
# (~/.local/share/nearbee/apb-config), NEARBEE_ENV_FILE (.env at the
# repository root), NEARBEE_APB_ENV_VARS (variables passed from that file to
# apb and its executors, default NEARAI_API_KEY). Only the listed variables are
# copied, read the way Nearbee reads .env (scripts/dotenv-export.py), into a
# 0600 file under $XDG_RUNTIME_DIR; values are never printed. No OPENCODE_
# variable is ever passed: it would override the provider routes the tier
# check reads from opencode.json. NEARBEE_BIN (default: nearbee on PATH) is
# set in the unit for the repo-audit playbook's script nodes.
# NEARBEE_APB_XDG_CONFIG_HOME (~/.local/share/nearbee/xdg-config) is the
# unit's XDG_CONFIG_HOME: a directory Nearbee owns, created empty, so
# OpenCode in a run merges no global config (MCP servers, plugins, share,
# providers) over the workspace's opencode.json; start refuses when it holds
# an opencode entry. Set apb.xdg_config_home in nearbee.yaml to the same
# path when you change it: the tier check compares the two.
set -euo pipefail

UNIT="${NEARBEE_APB_UNIT:-nearbee-apb}"
SANDBOX="${NEARBEE_APB_SANDBOX:-0}"
PORT="${NEARBEE_APB_PORT:-7322}"
CONFIG_DIR="${NEARBEE_APB_CONFIG_DIR:-$HOME/.local/share/nearbee/apb-config}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${NEARBEE_ENV_FILE:-$REPO_ROOT/.env}"
ENV_VARS="${NEARBEE_APB_ENV_VARS:-NEARAI_API_KEY}"
# The names as an array: split on whitespace, never globbed.
read -ra ENV_NAMES <<< "$ENV_VARS"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-$HOME/.cache}"
APB_ENV="$RUNTIME_DIR/nearbee-apb.env"
APB_BIN="${APB_BIN:-$(command -v apb || true)}"
XDG_DIR="${NEARBEE_APB_XDG_CONFIG_HOME:-$HOME/.local/share/nearbee/xdg-config}"

die() { echo "apb-instance: $*" >&2; exit 1; }

# The nearbee binary the repo-audit script nodes run: NEARBEE_BIN, else
# nearbee on PATH; empty when neither is found.
nearbee_bin() {
  if [ -n "${NEARBEE_BIN:-}" ]; then printf '%s' "$NEARBEE_BIN"; return; fi
  command -v nearbee 2>/dev/null || true
}

# OpenCode reads OPENCODE_ variables over its config files, and
# NEARAI_CLOUD_URL, NEARBEE_CLOUD_* or a proxy variable could redirect a
# direct model call of Nearbee's commands; the tier check treats any of
# them in the instance's environment as unknown routes, so none may reach
# apb (proxy and cloud names in any case).
redirecting_name() {
  local upper="${1^^}"
  case "$upper" in
    OPENCODE_*|NEARAI_CLOUD_URL|NEARBEE_CLOUD_*|HTTP_PROXY|HTTPS_PROXY|ALL_PROXY) return 0 ;;
  esac
  return 1
}

refuse_opencode_vars() {
  local name
  for name in "${ENV_NAMES[@]}"; do
    [[ "$name" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "NEARBEE_APB_ENV_VARS names $name, which is not a variable name"
    if redirecting_name "$name"; then
      die "NEARBEE_APB_ENV_VARS names $name; no OPENCODE_, NEARAI_CLOUD_URL, NEARBEE_CLOUD_ or proxy variable may reach apb"
    fi
  done
  if have_systemd; then
    local found="" var
    for var in $(systemctl --user show-environment 2>/dev/null | sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p'); do
      if redirecting_name "$var"; then found="$found$var "; fi
    done
    [ -z "$found" ] || die "the systemd user manager's environment sets $found; unset it (systemctl --user unset-environment) before starting"
  fi
}

# The unit's XDG_CONFIG_HOME: created empty and private; an opencode entry
# in it would be merged into every OpenCode run, so start refuses it.
prepare_xdg() {
  mkdir -p "$XDG_DIR"
  chmod 700 "$XDG_DIR"
  if [ -e "$XDG_DIR/opencode" ] || [ -L "$XDG_DIR/opencode" ]; then
    die "$XDG_DIR holds an opencode entry; Nearbee's XDG config directory must hold no OpenCode configuration"
  fi
}

[ "$PORT" = 7321 ] && die "port 7321 is the operator's everyday apb; Nearbee uses its own port"
if [ "$SANDBOX" = 1 ]; then
  # shellcheck source=scripts/apb-sandbox.sh
  . "$REPO_ROOT/scripts/apb-sandbox.sh"
else
  [ -n "$APB_BIN" ] || die "apb is not on PATH (set APB_BIN)"
fi

# Runs the apb CLI of this instance from directory $1: on the host, or in
# the sandbox (one-off container or the running dashboard container).
apb_at() {
  local dir="$1"
  shift
  if [ "$SANDBOX" = 1 ]; then
    sbx_apb "$dir" dashboard "$@"
  else
    ( cd "$dir" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" "$@" )
  fi
}

# The temp key file being written, removed if the script fails or is
# interrupted before it is moved into place.
ENV_TMP=""
drop_env_tmp() { if [ -n "$ENV_TMP" ]; then rm -f "$ENV_TMP"; ENV_TMP=""; fi; }

write_env() {
  # A fresh 0600 file moved over the old one: an existing file keeps no
  # wider mode it had, and a reader never sees a half-written file.
  mkdir -p "$RUNTIME_DIR"
  local tmp
  trap drop_env_tmp EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  ENV_TMP="$(umask 077; mktemp "$RUNTIME_DIR/nearbee-apb.env.XXXXXX")" \
    || die "cannot create a key file in $RUNTIME_DIR"
  tmp="$ENV_TMP"
  if [ -f "$ENV_FILE" ]; then
    # Read the way Nearbee reads it (dotenvy), written double-quoted for
    # systemd's EnvironmentFile and the shell alike.
    python3 "$REPO_ROOT/scripts/dotenv-export.py" "$ENV_FILE" "$tmp" "${ENV_NAMES[@]}" \
      || die "no key file written: $ENV_FILE cannot be used"
  else
    echo "apb-instance: no $ENV_FILE; starting without provider keys" >&2
  fi
  chmod 600 "$tmp"
  mv -f "$tmp" "$APB_ENV"
  ENV_TMP=""
  trap - EXIT HUP INT TERM
}

# Writes the install payload (JSON on stdin) into workspace $1 through
# scripts/apb-workspace-write.py: inside the sandbox in sandbox mode, on the
# host otherwise.
ws_write() {
  if [ "$SANDBOX" = 1 ]; then
    sbx_exec "$1" dashboard python3 -c "$(cat "$REPO_ROOT/scripts/apb-workspace-write.py")" "$(real "$1")"
  else
    python3 "$REPO_ROOT/scripts/apb-workspace-write.py" "$1"
  fi
}

# Writes stdin to the policy file $1 in place (the inode a running sandbox
# binds read-only stays the same), refusing a symlink and a file whose
# first line is not this script's marker.
write_policy() {
  local text
  text="$(cat)"
  POLICY_TEXT="$text" python3 -c '
import os, sys
path = sys.argv[1]
text = os.environ["POLICY_TEXT"] + "\n"
try:
    fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
except FileNotFoundError:
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
except OSError as e:
    sys.exit(f"apb-instance: {path} is a symlink or cannot be opened ({e.strerror}); not written")
with os.fdopen(fd, "r+", encoding="utf-8") as f:
    old = f.read()
    if old and not old.startswith("# nearbee: "):
        sys.exit(f"apb-instance: {path} exists and was not written by this script; edit it by hand")
    f.seek(0)
    f.truncate()
    f.write(text)
' "$1"
}

have_systemd() { command -v systemd-run >/dev/null && systemctl --user show-environment >/dev/null 2>&1; }

health() { curl -s -o /dev/null -w '%{http_code}' --max-time 3 "http://127.0.0.1:$PORT/api/health" || true; }

if [ "$SANDBOX" = 1 ]; then
  case "${1:-}" in
    start) sbx_start; exit 0 ;;
    stop) sbx_stop; exit 0 ;;
    down) sbx_down; exit 0 ;;
    status) sbx_status; exit 0 ;;
    apb)
      shift
      case "${1:-} ${2:-} ${3:-}" in
        "server key issue"|"server key revoke")
          # The key file is bound read-only into a running dashboard.
          ! sbx_running || die "stop the instance before changing its server keys"
          sbx_need_docker; sbx_prepare_dirs
          sbx_apb "$PWD" admin "$@"; exit $? ;;
      esac
      sbx_need_docker; sbx_prepare_dirs
      sbx_apb "$PWD" dashboard "$@"; exit $? ;;
    register|install) sbx_need_docker; sbx_prepare_dirs ;;
  esac
elif [ "${1:-}" = down ]; then
  die "down is for the sandbox mode (NEARBEE_APB_SANDBOX=1)"
fi

case "${1:-}" in
  start)
    refuse_opencode_vars
    prepare_xdg
    mkdir -p "$CONFIG_DIR"
    chmod 700 "$CONFIG_DIR"
    write_env
    bin="$(nearbee_bin)"
    [ -n "$bin" ] || echo "apb-instance: nearbee is not on PATH and NEARBEE_BIN is not set; the repo-audit scripts will fail" >&2
    if ! have_systemd; then
      echo "No systemd user session. Run instead (with no OPENCODE_ variable set):"
      echo "  set -a; . $APB_ENV; set +a; NEARBEE_BIN=$bin APB_CONFIG_DIR=$CONFIG_DIR XDG_CONFIG_HOME=$XDG_DIR $APB_BIN dashboard --no-open --port $PORT"
      exit 0
    fi
    if systemctl --user is-active --quiet "$UNIT"; then
      echo "apb-instance: $UNIT already running"; exit 0
    fi
    systemctl --user reset-failed "$UNIT" 2>/dev/null || true
    # KillMode=process: stopping the dashboard leaves run drivers alone
    # (apb docs/DEPLOYMENT.md, "Run it as a service").
    systemd-run --user --unit "$UNIT" --collect \
      -p KillMode=process -p "EnvironmentFile=$APB_ENV" \
      --setenv=APB_CONFIG_DIR="$CONFIG_DIR" --setenv=PATH="$PATH" --setenv=HOME="$HOME" \
      --setenv=NEARBEE_BIN="$bin" --setenv=XDG_CONFIG_HOME="$XDG_DIR" \
      --working-directory="$CONFIG_DIR" \
      "$APB_BIN" dashboard --no-open --port "$PORT" >/dev/null
    for _ in $(seq 1 50); do
      [ "$(health)" = 200 ] && { echo "apb-instance: up on http://127.0.0.1:$PORT"; exit 0; }
      sleep 0.2
    done
    die "the dashboard did not answer on port $PORT; see: journalctl --user -u $UNIT"
    ;;
  stop)
    if have_systemd; then systemctl --user stop "$UNIT" 2>/dev/null || true; fi
    # Also temp key files a start that was killed outright left behind.
    rm -f "$APB_ENV" "$RUNTIME_DIR"/nearbee-apb.env.*
    echo "apb-instance: stopped"
    ;;
  status)
    if have_systemd; then echo "unit: $(systemctl --user is-active "$UNIT" 2>/dev/null || true)"; fi
    echo "health http://127.0.0.1:$PORT/api/health: $(health)"
    echo "config dir: $CONFIG_DIR"
    ;;
  register)
    dir="${2:?usage: register <dir> [--git]}"
    # In sandbox mode the directory is shared with steps: it is made, and
    # git runs in it, inside the container (git would run hooks and
    # follow links a step planted there).
    if [ "$SANDBOX" = 1 ]; then
      sbx_register "$dir" "${3:-}"
    else
      mkdir -p "$dir"
      if [ "${3:-}" = "--git" ] && [ ! -d "$dir/.git" ]; then
        git -C "$dir" init -q
        git -C "$dir" -c user.name=nearbee -c user.email=nearbee@nearbee.local \
          commit -q --allow-empty -m init
      fi
    fi
    apb_at "$dir" init --standing-block no --feedback-loop no >/dev/null
    # Any registering command in the directory adds it to this instance's registry.
    apb_at "$dir" list >/dev/null
    echo "apb-instance: registered $dir"
    ;;
  install)
    # This logic is mirrored in Rust by nearbee-apb::install (the publish
    # path); keep the two in step, the safety rules especially.
    ws="${2:-${NEARBEE_APB_WORKSPACE:-$HOME/.local/share/nearbee/apb-workspace}}"
    [ -d "$ws/.apb" ] || die "$ws is not an apb project; run: $0 register $ws --git"
    if [ "$SANDBOX" = 1 ]; then sbx_require_mounted "$ws"; fi
    # Every workspace write goes through scripts/apb-workspace-write.py,
    # which follows no symlink; in sandbox mode it runs inside the
    # container, so it reaches only what a step could reach anyway.
    # Profiles become project profiles; OpenCode reads its provider config
    # from the project directory; decisions are off for the whole project
    # (design 2, point 4: the tier check reads decisions.enabled, and an
    # attested listing refuses any value but false). Other keys of
    # .apb/config.yaml are kept; an earlier decisions block is replaced.
    python3 - "$REPO_ROOT" <<'PY' | ws_write "$ws" || die "the workspace files were not written"
import json, os, sys
root = sys.argv[1]
files = {}
pdir = os.path.join(root, "playbooks/profiles")
for name in sorted(os.listdir(pdir)):
    if not name.endswith(".yaml"):
        continue
    stem = name[: -len(".yaml")]
    text = "".join(l for l in open(os.path.join(pdir, name), encoding="utf-8") if not l.startswith("#"))
    files[f".apb/profiles/{stem}/profile.yaml"] = text
    files[f".apb/profiles/{stem}/SOUL.md"] = "Do exactly the task in the prompt and nothing else.\n"
files["opencode.json"] = open(os.path.join(root, "playbooks/workspace/opencode.json"), encoding="utf-8").read()
json.dump({"files": files, "decisions_off": True}, sys.stdout)
PY
    for f in "$REPO_ROOT"/playbooks/profiles/*.yaml; do
      echo "apb-instance: profile $(basename "$f" .yaml)"
    done
    echo "apb-instance: opencode.json (provider nearai)"
    echo "apb-instance: decisions off in $ws/.apb/config.yaml"
    # The bundles are built in a private host directory (not mounted into
    # the sandbox) and reach apb only on stdin; the accept digest is the
    # sha256 of those bytes, computed here, never read from apb's output.
    mkdir -p "$RUNTIME_DIR"
    bundles="$(umask 077; mktemp -d "$RUNTIME_DIR/nearbee-bundles.XXXXXX")"
    # Whether apb's current version of <id> is <ver> and holds exactly the
    # bundle just built (apb's own comparison of definition and scripts).
    holds_bundle() {
      local answer
      answer="$(apb_at "$ws" import /dev/stdin --preview < "$bundles/$1.json" 2>&1)" || return 1
      printf '%s\n' "$answer" | grep -qxF "bundle $1 @ $2: unchanged: the current version already holds this bundle"
    }
    trap 'rm -rf "$bundles"' EXIT
    for f in "$REPO_ROOT"/playbooks/smoke/*.yaml; do
      id="$(basename "$f" .yaml)"
      python3 - "$f" "$REPO_ROOT/playbooks/smoke/scripts" "$bundles/$id.json" <<'PY'
import base64, json, os, re, sys
src, scripts, out = sys.argv[1:4]
text = open(src).read()
pid = re.search(r"^id: (\S+)$", text, re.M).group(1)
ver = re.search(r"^version: (\S+)$", text, re.M).group(1)
files = []
for name in sorted(os.listdir(scripts)):
    p = os.path.join(scripts, name)
    files.append({"path": name, "content_base64": base64.b64encode(open(p, "rb").read()).decode(),
                  "executable": os.access(p, os.X_OK)})
json.dump({"apb_bundle": 2, "id": pid, "version": ver, "playbook": text, "scripts": files}, open(out, "w"))
PY
      ver="$(sed -n 's/^version: //p' "$f" | head -n1)"
      preview="$(apb_at "$ws" import /dev/stdin < "$bundles/$id.json" 2>&1 || true)"
      if printf '%s\n' "$preview" | grep -qx 'nothing to import'; then
        # apb compared the bundle with its current version: same definition
        # and same scripts.
        echo "apb-instance: playbook $id unchanged"
      elif printf '%s\n' "$preview" | grep -q 'already exists'; then
        # The same version with other content: approving it would approve
        # content this script did not build.
        printf '%s\n' "$preview" >&2
        die "playbook $id $ver exists in the workspace with other content; bump the version in playbooks/smoke/$id.yaml"
      else
        accept="sha256:$(sha256sum "$bundles/$id.json" | cut -d' ' -f1)"
        answer="$(apb_at "$ws" import /dev/stdin --accept "$accept" < "$bundles/$id.json" 2>&1)" \
          || { printf '%s\n' "$preview" "$answer" >&2; die "import of $id refused"; }
      fi
      # Only the content just built is approved: apb must hold the bundle as
      # the current version before the approval, and still after it.
      holds_bundle "$id" "$ver" || die "playbook $id $ver in the workspace is not the bundle just built; nothing approved"
      # Approve every version, also an unchanged one (an earlier install may
      # have stopped before its approval). Without --digest apb approves a
      # version without scripts (exit 0) or prints the trust digest to review
      # (exit 2); anything else leaves the version unapproved and stops here.
      digest=""
      rc=0
      answer="$(apb_at "$ws" trust approve "$id" --version "$ver" 2>&1)" || rc=$?
      if [ "$rc" -ne 0 ]; then
        digest="$(printf '%s\n' "$answer" | sed -n 's/.*--digest \(sha256:[0-9a-f]*\).*/\1/p' | head -n1)"
        if [ -z "$digest" ]; then
          printf '%s\n' "$answer" >&2
          die "playbook $id $ver not approved: apb printed no trust digest"
        fi
        rc=0
        answer="$(apb_at "$ws" trust approve "$id" --version "$ver" --digest "$digest" 2>&1)" || rc=$?
        if [ "$rc" -ne 0 ]; then
          printf '%s\n' "$answer" >&2
          die "playbook $id $ver not approved: apb refused digest $digest"
        fi
      fi
      if ! holds_bundle "$id" "$ver"; then
        if [ -n "$digest" ]; then
          apb_at "$ws" trust revoke "$digest" >/dev/null 2>&1 || true
        else
          apb_at "$ws" trust revoke "$id" --kind playbook >/dev/null 2>&1 || true
        fi
        die "playbook $id $ver changed during its approval; the approval was revoked"
      fi
      echo "apb-instance: playbook $id $ver approved"
    done
    ;;
  allow-billed)
    mkdir -p "$CONFIG_DIR"
    # Written in place (a running sandbox binds this very file read-only)
    # and never through a symlink; refused when this script did not write
    # it.
    write_policy "$CONFIG_DIR/policy.yaml" <<'YAML'
# nearbee: allow-billed (scripts/apb-instance.sh). Nearbee's own apb instance
# starts billed (NEAR AI Cloud) agent work without a per-run confirmation;
# each run's spend is capped by the budget Nearbee passes.
billing:
  billed: allow
  unknown_basis: allow
YAML
    echo "apb-instance: billed work allowed in $CONFIG_DIR/policy.yaml"
    ;;
  apb)
    shift
    APB_CONFIG_DIR="$CONFIG_DIR" exec "$APB_BIN" "$@"
    ;;
  *)
    sed -n '2,40p' "$0"; exit 2 ;;
esac
