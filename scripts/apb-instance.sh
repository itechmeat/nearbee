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
#                                      with their scripts and approved by their trust digest
#   scripts/apb-instance.sh allow-billed
#                                      let this instance start billed and unknown-basis agent
#                                      work (NEAR AI Cloud through OpenCode) without a per-run
#                                      confirmation: writes billing: allow to its policy.yaml.
#                                      Only on the owner's go-ahead for Cloud spend (O-3, O-6);
#                                      Nearbee's runner never confirms a launch gate itself
#   scripts/apb-instance.sh apb <args> run the apb CLI against this instance's config
#
# Settings (environment): NEARBEE_APB_PORT (7322), NEARBEE_APB_CONFIG_DIR
# (~/.local/share/nearbee/apb-config), NEARBEE_ENV_FILE (.env at the
# repository root), NEARBEE_APB_ENV_VARS (variables passed from that file to
# apb and its executors, default NEARAI_API_KEY). Only the listed variables are
# copied, read the way Nearbee reads .env (scripts/dotenv-export.py), into a
# 0600 file under $XDG_RUNTIME_DIR; values are never printed.
set -euo pipefail

UNIT=nearbee-apb
PORT="${NEARBEE_APB_PORT:-7322}"
CONFIG_DIR="${NEARBEE_APB_CONFIG_DIR:-$HOME/.local/share/nearbee/apb-config}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${NEARBEE_ENV_FILE:-$REPO_ROOT/.env}"
ENV_VARS="${NEARBEE_APB_ENV_VARS:-NEARAI_API_KEY}"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-$HOME/.cache}"
APB_ENV="$RUNTIME_DIR/nearbee-apb.env"
APB_BIN="${APB_BIN:-$(command -v apb || true)}"

die() { echo "apb-instance: $*" >&2; exit 1; }

[ "$PORT" = 7321 ] && die "port 7321 is the operator's everyday apb; Nearbee uses its own port"
[ -n "$APB_BIN" ] || die "apb is not on PATH (set APB_BIN)"

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
    # shellcheck disable=SC2086 # ENV_VARS is a list of names
    python3 "$REPO_ROOT/scripts/dotenv-export.py" "$ENV_FILE" "$tmp" $ENV_VARS \
      || die "no key file written: $ENV_FILE cannot be used"
  else
    echo "apb-instance: no $ENV_FILE; starting without provider keys" >&2
  fi
  chmod 600 "$tmp"
  mv -f "$tmp" "$APB_ENV"
  ENV_TMP=""
  trap - EXIT HUP INT TERM
}

have_systemd() { command -v systemd-run >/dev/null && systemctl --user show-environment >/dev/null 2>&1; }

health() { curl -s -o /dev/null -w '%{http_code}' --max-time 3 "http://127.0.0.1:$PORT/api/health" || true; }

case "${1:-}" in
  start)
    mkdir -p "$CONFIG_DIR"
    chmod 700 "$CONFIG_DIR"
    write_env
    if ! have_systemd; then
      echo "No systemd user session. Run instead:"
      echo "  set -a; . $APB_ENV; set +a; APB_CONFIG_DIR=$CONFIG_DIR $APB_BIN dashboard --no-open --port $PORT"
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
    mkdir -p "$dir"
    if [ "${3:-}" = "--git" ] && [ ! -d "$dir/.git" ]; then
      git -C "$dir" init -q
      git -C "$dir" -c user.name=nearbee -c user.email=nearbee@nearbee.local \
        commit -q --allow-empty -m init
    fi
    ( cd "$dir" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" init --standing-block no --feedback-loop no >/dev/null )
    # Any registering command in the directory adds it to this instance's registry.
    ( cd "$dir" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" list >/dev/null )
    echo "apb-instance: registered $dir"
    ;;
  install)
    # This logic is mirrored in Rust by nearbee-apb::install (the publish
    # path); keep the two in step, the safety rules especially.
    ws="${2:-$HOME/.local/share/nearbee/apb-workspace}"
    [ -d "$ws/.apb" ] || die "$ws is not an apb project; run: $0 register $ws --git"
    for f in "$REPO_ROOT"/playbooks/profiles/*.yaml; do
      name="$(basename "$f" .yaml)"
      mkdir -p "$ws/.apb/profiles/$name"
      grep -v '^#' "$f" > "$ws/.apb/profiles/$name/profile.yaml"
      printf 'Do exactly the task in the prompt and nothing else.\n' > "$ws/.apb/profiles/$name/SOUL.md"
      echo "apb-instance: profile $name"
    done
    # OpenCode reads its provider config from the project directory.
    cp "$REPO_ROOT/playbooks/workspace/opencode.json" "$ws/opencode.json"
    echo "apb-instance: opencode.json (provider nearai)"
    bundles="$(mktemp -d "${TMPDIR:-/tmp}/nearbee-bundles.XXXXXX")"
    # Whether apb's current version of <id> is <ver> and holds exactly the
    # bundle just built (apb's own comparison of definition and scripts).
    holds_bundle() {
      local answer
      answer="$(cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" import "$bundles/$1.json" --preview 2>&1)" || return 1
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
      preview="$(cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" import "$bundles/$id.json" 2>&1 || true)"
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
        accept="$(printf '%s' "$preview" | sed -n 's/.*--accept \(sha256:[0-9a-f]*\).*/\1/p' | head -n1)"
        [ -n "$accept" ] || { printf '%s\n' "$preview" >&2; die "import of $id refused"; }
        ( cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" import "$bundles/$id.json" --accept "$accept" >/dev/null )
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
      answer="$(cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" trust approve "$id" --version "$ver" 2>&1)" || rc=$?
      if [ "$rc" -ne 0 ]; then
        digest="$(printf '%s\n' "$answer" | sed -n 's/.*--digest \(sha256:[0-9a-f]*\).*/\1/p' | head -n1)"
        if [ -z "$digest" ]; then
          printf '%s\n' "$answer" >&2
          die "playbook $id $ver not approved: apb printed no trust digest"
        fi
        rc=0
        answer="$(cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" trust approve "$id" --version "$ver" --digest "$digest" 2>&1)" || rc=$?
        if [ "$rc" -ne 0 ]; then
          printf '%s\n' "$answer" >&2
          die "playbook $id $ver not approved: apb refused digest $digest"
        fi
      fi
      if ! holds_bundle "$id" "$ver"; then
        if [ -n "$digest" ]; then
          ( cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" trust revoke "$digest" >/dev/null 2>&1 ) || true
        else
          ( cd "$ws" && APB_CONFIG_DIR="$CONFIG_DIR" "$APB_BIN" trust revoke "$id" --kind playbook >/dev/null 2>&1 ) || true
        fi
        die "playbook $id $ver changed during its approval; the approval was revoked"
      fi
      echo "apb-instance: playbook $id $ver approved"
    done
    ;;
  allow-billed)
    mkdir -p "$CONFIG_DIR"
    if [ -f "$CONFIG_DIR/policy.yaml" ] && ! grep -q '^# nearbee: allow-billed' "$CONFIG_DIR/policy.yaml"; then
      die "$CONFIG_DIR/policy.yaml exists and was not written by this script; edit it by hand"
    fi
    cat > "$CONFIG_DIR/policy.yaml" <<'YAML'
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
    sed -n '2,29p' "$0"; exit 2 ;;
esac
