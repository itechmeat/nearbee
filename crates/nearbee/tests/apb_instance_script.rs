//! `scripts/apb-instance.sh` against a fake `apb`: `install` approves only
//! the content it just built, by the digests apb printed, or fails and says
//! why; `start` writes the provider key file as a fresh 0600 file.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FAKE_APB: &str = r#"#!/usr/bin/env bash
# Fake apb for the install test. It approves only the digest it printed and
# imports only the bundle digest it printed, and logs every call.
# FAKE_TRUST: digest | nodigest | plain. FAKE_IMPORT: new | unchanged | differ
# (the version exists with other content). FAKE_SWAP=1: the stored version
# changes right after its approval.
# An import reads its bundle only from stdin (/dev/stdin) and accepts only
# the sha256 of the bytes it read; the digest it prints is a wrong one, so
# only a caller that computes the digest itself gets an import through.
state="$(dirname "$FAKE_LOG")/state"; mkdir -p "$state"
bundle_field() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1" "$2"; }
if [ "$1" != import ]; then echo "$*" >> "$FAKE_LOG"; fi
case "$1" in
  import)
    if [ "$2" != /dev/stdin ]; then
      echo "import $*" >> "$FAKE_LOG"
      echo "fake: the bundle must come on stdin, not from $2" >&2; exit 2
    fi
    file="$state/in.$$"; cat > "$file"; trap 'rm -f "$file"' EXIT
    id="$(bundle_field "$file" id)"; ver="$(bundle_field "$file" version)"
    digest="sha256:$(sha256sum "$file" | cut -d' ' -f1)"
    shift 2
    printf 'import %s %s\n' "$id" "$*" >> "$FAKE_LOG"
    stored=no
    case "${FAKE_IMPORT:-new}" in
      unchanged) stored=yes ;;
      new) [ -e "$state/$id.imported" ] && stored=yes ;;
    esac
    [ -e "$state/$id.swapped" ] && stored=no
    case "$*" in
      *--preview*)
        if [ "$stored" = yes ]; then
          echo "bundle $id @ $ver: unchanged: the current version already holds this bundle"
        else
          echo "bundle $id @ $ver: new version of an existing playbook"
        fi
        exit 0 ;;
      *"--accept $digest"*) touch "$state/$id.imported"; exit 0 ;;
      *--accept*) echo "import refused: --accept does not name this file's bundle digest" >&2; exit 2 ;;
    esac
    if [ "$stored" = yes ]; then echo "nothing to import"; exit 0; fi
    if [ "${FAKE_IMPORT:-new}" = differ ]; then
      echo "the version $ver already exists: the import would be refused"
      echo "import refused: see the preview above; nothing imported" >&2; exit 2
    fi
    echo "nothing imported: review the preview, then accept this exact file with --accept sha256:aaaa" >&2
    exit 3 ;;
  trust)
    [ "$2" = revoke ] && { echo "revoked"; exit 0; }
    id="$3"
    case "$*" in
      *"--digest sha256:bbbb"*)
        [ "${FAKE_SWAP:-}" = 1 ] && touch "$state/$id.swapped"
        echo "approved"; exit 0 ;;
      *--digest*) echo "refused: the digest does not match" >&2; exit 2 ;;
    esac
    case "${FAKE_TRUST:-digest}" in
      digest) printf 'scripts to review\nrun: apb trust approve x --digest sha256:bbbb\n'; exit 2 ;;
      nodigest) echo "error: something went wrong"; exit 2 ;;
      plain)
        [ "${FAKE_SWAP:-}" = 1 ] && touch "$state/$id.swapped"
        echo "approved"; exit 0 ;;
    esac ;;
esac
exit 0
"#;

struct Fake {
    dir: tempfile::TempDir,
}

impl Fake {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("apb");
        std::fs::write(&bin, FAKE_APB).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("ws/.apb")).unwrap();
        std::fs::create_dir_all(dir.path().join("tmp")).unwrap();
        Self { dir }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.path("log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn install(&self, trust: &str, import: &str) -> Output {
        self.install_with(trust, import, &[])
    }

    fn install_with(&self, trust: &str, import: &str, extra: &[(&str, &str)]) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-instance.sh");
        Command::new("bash")
            .arg(script)
            .arg("install")
            .arg(self.path("ws"))
            .env("APB_BIN", self.path("apb"))
            .env("FAKE_LOG", self.path("log"))
            .env("FAKE_TRUST", trust)
            .env("FAKE_IMPORT", import)
            .env("HOME", self.dir.path())
            .env("TMPDIR", self.path("tmp"))
            .env("NEARBEE_APB_CONFIG_DIR", self.path("config"))
            .envs(extra.iter().copied())
            .output()
            .unwrap()
    }
}

fn smoke_ids() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playbooks/smoke");
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "yaml")
                .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    ids.sort();
    assert!(!ids.is_empty());
    ids
}

fn smoke_version(id: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../playbooks/smoke/{id}.yaml"));
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("version: ").map(str::to_owned))
        .unwrap()
}

fn assert_ok(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn assert_reported_approved(stdout: &str) {
    for id in smoke_ids() {
        assert!(
            stdout
                .lines()
                .any(|l| l.contains(&format!("playbook {id}")) && l.contains("approved")),
            "{id} not reported approved:\n{stdout}"
        );
    }
}

/// The log lines of one kind of call for playbook `id`.
fn calls<'a>(log: &'a [String], prefix: &str, id: &str) -> Vec<&'a str> {
    log.iter()
        .map(String::as_str)
        .filter(|l| {
            l.starts_with(prefix)
                && (l.contains(&format!("/{id}.json")) || l.contains(&format!(" {id} ")))
        })
        .collect()
}

#[test]
fn install_imports_and_approves_every_playbook_by_its_own_bundle_digest() {
    let f = Fake::new();
    let stdout = assert_ok(&f.install("digest", "new"));
    assert_reported_approved(&stdout);
    let log = f.log();
    for id in smoke_ids() {
        let ver = smoke_version(&id);
        let imports = calls(&log, "import ", &id);
        // The accept names the digest of the bundle computed by the script
        // (the fake accepts only that), never the one apb printed.
        assert_eq!(
            imports
                .iter()
                .filter(|l| l.contains("--accept sha256:"))
                .count(),
            1,
            "{id}: one import accepting the bundle digest: {imports:?}"
        );
        assert!(
            !imports.iter().any(|l| l.contains("sha256:aaaa")),
            "{id}: {imports:?}"
        );
        let approvals = calls(&log, "trust approve ", &id);
        assert_eq!(
            approvals
                .iter()
                .filter(
                    |l| **l == format!("trust approve {id} --version {ver} --digest sha256:bbbb")
                )
                .count(),
            1,
            "{id}: one approval naming the printed trust digest: {approvals:?}"
        );
        assert!(
            !approvals
                .iter()
                .any(|l| l.contains("--digest") && !l.contains("sha256:bbbb")),
            "{approvals:?}"
        );
        // The stored version was compared with the fresh bundle before and
        // after the approval.
        assert_eq!(
            imports.iter().filter(|l| l.ends_with("--preview")).count(),
            2,
            "{id}: {imports:?}"
        );
    }
    assert!(
        !log.iter().any(|l| l.starts_with("trust revoke")),
        "{log:?}"
    );
}

#[test]
fn install_fails_when_no_trust_digest_is_found() {
    let f = Fake::new();
    let out = f.install("nodigest", "new");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "install succeeded without approval");
    let first = &smoke_ids()[0];
    assert!(
        stderr.contains(first) && stderr.contains("not approved"),
        "{stderr}"
    );
}

#[test]
fn install_approves_a_version_without_scripts_and_an_unchanged_import() {
    let f = Fake::new();
    let stdout = assert_ok(&f.install("plain", "unchanged"));
    assert_reported_approved(&stdout);
    let log = f.log();
    for id in smoke_ids() {
        assert!(
            !calls(&log, "import ", &id)
                .iter()
                .any(|l| l.contains("--accept")),
            "{id}: an unchanged bundle is not imported again"
        );
    }
}

#[test]
fn install_refuses_a_stored_version_with_other_content_and_approves_nothing() {
    // The workspace holds the same version with other content (apb answers
    // "already exists"): approving it would approve content nobody built.
    let f = Fake::new();
    let out = f.install("digest", "differ");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "install approved other content");
    let first = &smoke_ids()[0];
    assert!(stderr.contains(first), "{stderr}");
    assert!(stderr.contains("bump the version"), "{stderr}");
    assert!(
        !f.log().iter().any(|l| l.starts_with("trust approve")),
        "{:?}",
        f.log()
    );
}

#[test]
fn install_revokes_and_fails_when_the_stored_version_changes_during_approval() {
    for trust in ["digest", "plain"] {
        let f = Fake::new();
        let out = f.install_with(trust, "new", &[("FAKE_SWAP", "1")]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{trust}: install kept the approval");
        assert!(
            stderr.contains("changed during its approval"),
            "{trust}: {stderr}"
        );
        let first = &smoke_ids()[0];
        let revoke = if trust == "digest" {
            "trust revoke sha256:bbbb".to_owned()
        } else {
            format!("trust revoke {first} --kind playbook")
        };
        assert!(
            f.log().contains(&revoke),
            "{trust}: expected `{revoke}` in {:?}",
            f.log()
        );
    }
}

/// Names in `dir` that start with `nearbee-apb.env.` (temp key files).
fn temp_key_files(dir: &Path) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().starts_with("nearbee-apb.env."))
        .collect()
}

/// A PATH holding only the tools `start` needs, and no `systemd-run`, so
/// `start` prints the manual command instead of starting a unit.
fn tools_without_systemd(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in [
        "bash", "dirname", "mkdir", "chmod", "mktemp", "grep", "head", "sed", "mv", "rm", "cat",
        "python3",
    ] {
        let found = ["/usr/bin", "/bin"]
            .iter()
            .map(|d| Path::new(d).join(tool))
            .find(|p| p.exists())
            .unwrap_or_else(|| panic!("{tool} not found"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(found, bin.join(tool)).unwrap();
    }
    bin
}

#[cfg(unix)]
#[test]
fn start_rewrites_an_existing_wider_key_file_as_0600() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fake::new();
    let runtime = f.path("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let env_out = runtime.join("nearbee-apb.env");
    std::fs::write(&env_out, "OLD=1\n").unwrap();
    std::fs::set_permissions(&env_out, std::fs::Permissions::from_mode(0o644)).unwrap();
    let dotenv = f.path("dotenv");
    std::fs::write(&dotenv, "NEARAI_API_KEY=placeholder-not-a-key\nOTHER=x\n").unwrap();
    let path = tools_without_systemd(f.dir.path());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-instance.sh");
    let out = Command::new(path.join("bash"))
        .arg(script)
        .arg("start")
        .env_clear()
        .env("PATH", &path)
        .env("HOME", f.dir.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("APB_BIN", f.path("apb"))
        .env("NEARBEE_ENV_FILE", &dotenv)
        .env("NEARBEE_APB_CONFIG_DIR", f.path("config"))
        .output()
        .unwrap();
    assert_ok(&out);
    let mode = std::fs::metadata(&env_out).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "key file mode {mode:o}");
    assert_eq!(
        std::fs::read_to_string(&env_out).unwrap(),
        "NEARAI_API_KEY=\"placeholder-not-a-key\"\n"
    );
    let left: Vec<_> = std::fs::read_dir(&runtime)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        left,
        [std::ffi::OsString::from("nearbee-apb.env")],
        "no temp file left"
    );
}

#[cfg(unix)]
#[test]
fn start_whose_key_file_write_fails_leaves_no_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fake::new();
    let runtime = f.path("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let dotenv = f.path("dotenv");
    std::fs::write(&dotenv, "NEARAI_API_KEY=placeholder-not-a-key\n").unwrap();
    let path = tools_without_systemd(f.dir.path());
    // `mv` fails, so the temp file is never moved over the key file.
    std::fs::remove_file(path.join("mv")).unwrap();
    std::fs::write(path.join("mv"), "#!/bin/bash\nexit 1\n").unwrap();
    std::fs::set_permissions(path.join("mv"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-instance.sh");
    let out = Command::new(path.join("bash"))
        .arg(script)
        .arg("start")
        .env_clear()
        .env("PATH", &path)
        .env("HOME", f.dir.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("APB_BIN", f.path("apb"))
        .env("NEARBEE_ENV_FILE", &dotenv)
        .env("NEARBEE_APB_CONFIG_DIR", f.path("config"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "start succeeded with a failing mv");
    assert_eq!(temp_key_files(&runtime), Vec::<std::ffi::OsString>::new());
    assert!(!runtime.join("nearbee-apb.env").exists());
}

#[cfg(unix)]
#[test]
fn stop_removes_the_key_file_and_stale_temp_key_files() {
    let f = Fake::new();
    let runtime = f.path("run");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(runtime.join("nearbee-apb.env"), "K=1\n").unwrap();
    std::fs::write(runtime.join("nearbee-apb.env.Ab12Cd"), "K=1\n").unwrap();
    std::fs::write(runtime.join("other.env"), "K=1\n").unwrap();
    let path = tools_without_systemd(f.dir.path());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-instance.sh");
    let out = Command::new(path.join("bash"))
        .arg(script)
        .arg("stop")
        .env_clear()
        .env("PATH", &path)
        .env("HOME", f.dir.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("APB_BIN", f.path("apb"))
        .output()
        .unwrap();
    assert_ok(&out);
    assert_eq!(temp_key_files(&runtime), Vec::<std::ffi::OsString>::new());
    assert!(!runtime.join("nearbee-apb.env").exists());
    assert!(runtime.join("other.env").exists(), "only its own files");
}

/// Runs `start` (without systemd) on a `.env` with `text`, passing `names`;
/// returns the script's output and the key file it wrote.
#[cfg(unix)]
fn start_with_dotenv(text: &str, names: &str) -> (Output, Fake, PathBuf) {
    start_with_dotenv_env(text, names, &[])
}

fn start_with_dotenv_env(
    text: &str,
    names: &str,
    extra: &[(&str, &str)],
) -> (Output, Fake, PathBuf) {
    start_in(Fake::new(), text, names, extra)
}

/// [`start_with_dotenv_env`] in a prepared fake home.
fn start_in(f: Fake, text: &str, names: &str, extra: &[(&str, &str)]) -> (Output, Fake, PathBuf) {
    let runtime = f.path("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let dotenv = f.path("cfg/.env");
    std::fs::create_dir_all(f.path("cfg")).unwrap();
    std::fs::write(&dotenv, text).unwrap();
    let path = tools_without_systemd(f.dir.path());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-instance.sh");
    let out = Command::new(path.join("bash"))
        .arg(script)
        .arg("start")
        .env_clear()
        .env("PATH", &path)
        .env("HOME", f.dir.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("APB_BIN", f.path("apb"))
        .env("NEARBEE_ENV_FILE", &dotenv)
        .env("NEARBEE_APB_ENV_VARS", names)
        .env("NEARBEE_APB_CONFIG_DIR", f.path("config"))
        .envs(extra.iter().copied())
        .output()
        .unwrap();
    let key_file = runtime.join("nearbee-apb.env");
    (out, f, key_file)
}

/// The value of `name` after the shell sources `key_file` (what the manual
/// start command does; systemd reads the same quoting).
#[cfg(unix)]
fn sourced(key_file: &Path, name: &str) -> Option<String> {
    let out = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "set -a; . \"$1\"; set +a; [ -n \"${{{name}+x}}\" ] && printf %s \"${name}\""
        ))
        .arg("bash")
        .arg(key_file)
        .env_clear()
        .output()
        .unwrap();
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).unwrap())
}

/// The values Nearbee itself loads from the same `.env`.
fn nearbee_values(cfg_dir: &Path) -> (Option<String>, Option<String>) {
    let cfg = nearbee_core::config::load(
        nearbee_core::config::LoadOptions {
            file: None,
            cwd: cfg_dir,
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    (
        cfg.nearai_api_key.map(|s| s.expose().to_owned()),
        cfg.apb.api_key.map(|s| s.expose().to_owned()),
    )
}

#[cfg(unix)]
#[test]
fn start_passes_the_values_nearbee_reads_from_the_same_dotenv() {
    for (case, text) in [
        (
            "inline comment",
            "NEARAI_API_KEY=synthetic-key # operator note\nAPB_API_KEY=apb_x\n",
        ),
        (
            "whitespace-prefixed export",
            "   export NEARAI_API_KEY=synthetic-key\n\texport APB_API_KEY = apb_x\n",
        ),
        (
            "quotes and escapes",
            "NEARAI_API_KEY='lit $HOME `x` \\\\ \"q\"'\nAPB_API_KEY=\"a \\\"b\\\" c\\\\d\\$e\"\n",
        ),
        (
            "last assignment wins and substitution",
            "NEARAI_API_KEY=first\nBASE=abc\nNEARAI_API_KEY=second\nAPB_API_KEY=${BASE}-$BASE\n",
        ),
        (
            "multi-line quoted value",
            "NEARAI_API_KEY=\"line1\nline2\"\nAPB_API_KEY=apb_x\n",
        ),
    ] {
        let (out, f, key_file) = start_with_dotenv(text, "NEARAI_API_KEY APB_API_KEY");
        assert_ok(&out);
        let (nearai, apb) = nearbee_values(&f.path("cfg"));
        assert!(nearai.is_some() && apb.is_some(), "{case}");
        assert_eq!(sourced(&key_file, "NEARAI_API_KEY"), nearai, "{case}");
        assert_eq!(sourced(&key_file, "APB_API_KEY"), apb, "{case}");
        // Double-quoted, the one form systemd's EnvironmentFile and the
        // shell read alike (an unquoted ` # note` is a comment only to the
        // shell).
        let written = std::fs::read_to_string(&key_file).unwrap();
        assert!(
            written.starts_with("NEARAI_API_KEY=\"") && written.contains("\nAPB_API_KEY=\""),
            "{case}: not double-quoted"
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.contains("synthetic-key") && !stdout.contains("apb_x"),
            "{case}: {stdout}"
        );
    }
}

#[cfg(unix)]
#[test]
fn start_refuses_a_dotenv_nearbee_cannot_parse_without_printing_it() {
    let (out, _f, key_file) =
        start_with_dotenv("NEARAI_API_KEY=\"synthetic-unclosed\n", "NEARAI_API_KEY");
    assert!(!out.status.success(), "start accepted an unparsable .env");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("synthetic-unclosed"), "{all}");
    assert!(!key_file.exists());
}

#[cfg(unix)]
#[test]
fn start_treats_an_empty_value_as_not_set() {
    let (out, _f, key_file) = start_with_dotenv("NEARAI_API_KEY=\n", "NEARAI_API_KEY");
    let stdout = assert_ok(&out);
    assert!(stdout.contains("NEARAI_API_KEY is not set"), "{stdout}");
    assert_eq!(sourced(&key_file, "NEARAI_API_KEY"), None);
}

/// `install` installs every shipped profile, the premium one included,
/// as a project profile (task 9.4).
#[test]
fn install_installs_the_premium_profile() {
    let f = Fake::new();
    let stdout = assert_ok(&f.install("digest", "new"));
    for name in ["nearbee-own", "nearbee-own-premium", "nearbee-near"] {
        let profile = f.path(&format!("ws/.apb/profiles/{name}/profile.yaml"));
        let text = std::fs::read_to_string(&profile).unwrap();
        assert!(text.contains(&format!("name: {name}")), "{name}: {text}");
        assert!(
            stdout.contains(&format!("apb-instance: profile {name}")),
            "{stdout}"
        );
    }
}

/// `install` writes `decisions: { enabled: false }` into the project's
/// `.apb/config.yaml`, keeping the other keys and replacing an earlier
/// decisions block (design 2, point 4).
#[test]
fn install_turns_decisions_off_in_the_project_config() {
    let f = Fake::new();
    assert_ok(&f.install("digest", "new"));
    let text = std::fs::read_to_string(f.path("ws/.apb/config.yaml")).unwrap();
    let v: serde_json::Value = yaml_serde::from_str(&text).unwrap();
    assert_eq!(v["decisions"]["enabled"], false, "{text}");

    let f = Fake::new();
    std::fs::write(
        f.path("ws/.apb/config.yaml"),
        "feedback_loop: no\ndecisions:\n  enabled: true\n  provider: other\nstanding_block: no\n",
    )
    .unwrap();
    assert_ok(&f.install("digest", "new"));
    let text = std::fs::read_to_string(f.path("ws/.apb/config.yaml")).unwrap();
    let v: serde_json::Value = yaml_serde::from_str(&text).unwrap();
    assert_eq!(
        v["decisions"],
        serde_json::json!({"enabled": false}),
        "{text}"
    );
    assert_eq!(v["feedback_loop"], "no", "{text}");
    assert_eq!(v["standing_block"], "no", "{text}");
    assert_eq!(text.matches("decisions:").count(), 1, "{text}");
}

/// `start` sets `NEARBEE_BIN` for the repo-audit script nodes (task 9.4).
#[cfg(unix)]
#[test]
fn start_sets_nearbee_bin_for_the_script_nodes() {
    let (out, _f, _key) = start_with_dotenv_env(
        "NEARAI_API_KEY=synthetic-key\n",
        "NEARAI_API_KEY",
        &[("NEARBEE_BIN", "/opt/nearbee/bin/nearbee")],
    );
    let stdout = assert_ok(&out);
    assert!(
        stdout.contains("NEARBEE_BIN=/opt/nearbee/bin/nearbee "),
        "{stdout}"
    );
}

/// No `OPENCODE_` variable may reach apb: `start` refuses to pass one
/// from the dotenv and writes no key file (design 2, point 3).
#[cfg(unix)]
#[test]
fn start_refuses_to_pass_an_opencode_variable() {
    let (out, _f, key_file) = start_with_dotenv(
        "NEARAI_API_KEY=synthetic-key\nOPENCODE_CONFIG=/tmp/x.json\n",
        "NEARAI_API_KEY OPENCODE_CONFIG",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("OPENCODE_CONFIG"), "{stderr}");
    assert!(!key_file.exists(), "a key file was written");
}

/// No variable that could redirect a direct model call may reach apb
/// (task 3.2, spec `privacy-tiers`, Direct call route).
#[cfg(unix)]
#[test]
fn start_refuses_to_pass_a_redirecting_variable() {
    for name in [
        "NEARAI_CLOUD_URL",
        "NEARBEE_CLOUD_BASE",
        "https_proxy",
        "ALL_PROXY",
    ] {
        let (out, _f, key_file) = start_with_dotenv(
            &format!("NEARAI_API_KEY=synthetic-key\n{name}=http://x\n"),
            &format!("NEARAI_API_KEY {name}"),
        );
        assert!(!out.status.success(), "{name}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(name), "{stderr}");
        assert!(!key_file.exists(), "a key file was written");
    }
}

/// `start` gives the instance Nearbee's own `XDG_CONFIG_HOME`, created
/// empty and private, so OpenCode merges no global config (spec
/// `privacy-tiers`, OpenCode configuration).
#[cfg(unix)]
#[test]
fn start_sets_nearbees_own_xdg_config_home() {
    use std::os::unix::fs::PermissionsExt;
    let (out, f, _key) = start_with_dotenv("NEARAI_API_KEY=synthetic-key\n", "NEARAI_API_KEY");
    let stdout = assert_ok(&out);
    let xdg = f.path(".local/share/nearbee/xdg-config");
    assert!(
        stdout.contains(&format!("XDG_CONFIG_HOME={} ", xdg.display())),
        "{stdout}"
    );
    let meta = std::fs::metadata(&xdg).expect("the directory is created");
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::read_dir(&xdg).unwrap().count(), 0, "it is empty");
}

/// An `opencode` entry in Nearbee's XDG config directory would be merged
/// into every OpenCode run: `start` refuses and writes no key file.
#[cfg(unix)]
#[test]
fn start_refuses_an_opencode_config_in_the_xdg_directory() {
    let f = Fake::new();
    std::fs::create_dir_all(f.path(".local/share/nearbee/xdg-config/opencode")).unwrap();
    let (out, _f, key_file) = start_in(f, "NEARAI_API_KEY=synthetic-key\n", "NEARAI_API_KEY", &[]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("holds an opencode entry"), "{stderr}");
    assert!(!key_file.exists(), "a key file was written");
}

// Sandbox mode (change apb-sandbox) against a fake `docker` that logs its
// calls and answers from FAKE_* variables; no daemon, no container.

const FAKE_DOCKER: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FAKE_DOCKER_LOG"
case "$1" in
  info) exit 0 ;;
  network)
    case "$2" in
      inspect)
        case "$*" in
          *-int*)
            # FAKE_NET_INT: "<subnet>|<gateway mode>" of an existing network.
            [ -n "${FAKE_NET_INT:-}" ] || exit 1
            case "$*" in *" -f "*) printf '%s\n' "$FAKE_NET_INT" ;; esac
            exit 0 ;;
          *) [ -n "${FAKE_NET_OUT:-}" ] || exit 1; exit 0 ;;
        esac ;;
      connect) [ -z "${FAKE_CONNECT_FAIL:-}" ] || exit 1 ;;
    esac
    exit 0 ;;
  image)
    case "$*" in
      *apb-sha256*) printf '%s\n' "${FAKE_APB_SHA:-}" ;;
      *nearbee-sha256*) printf '%s\n' "${FAKE_NEARBEE_SHA:-}" ;;
    esac
    exit 0 ;;
  inspect)
    case "$*" in
      *State.Running*) printf '%s\n' "${FAKE_RUNNING:-false}" ;;
      *gateway-hash*) [ -n "${FAKE_GW_HASH:-}" ] || exit 1; printf '%s\n' "$FAKE_GW_HASH" ;;
      *IPAddress*) printf '%s\n' "${FAKE_GW_IP:-}" ;;
      *nearbee.config-dir*) printf '%s\n' "${FAKE_CONFIG_DIR:-}" ;;
      *) exit 1 ;;
    esac
    exit 0 ;;
  run|exec)
    cat > /dev/null
    case "$*" in
      *"server key list"*)
        if [ -n "${FAKE_KEY_LISTED:-}" ]; then printf 'ID        CREATED\nabcd1234  2026-11-12T00:00:00Z\n'
        else printf 'no server keys; the dashboard runs unauthenticated\n'; fi ;;
      *"server key issue"*) printf 'apb_fakeissuedkey0123456789\n\nkey id: abcd1234\n' ;;
    esac
    exit 0 ;;
esac
exit 0
"#;

struct Sandbox {
    f: Fake,
    path: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let f = Fake::new();
        let path = tools_without_systemd(f.dir.path());
        for tool in [
            "realpath",
            "sha256sum",
            "cut",
            "readlink",
            "ls",
            "id",
            "dirname",
            "tr",
            "sort",
            "env",
            "touch",
            "stat",
            "basename",
        ] {
            let found = ["/usr/bin", "/bin"]
                .iter()
                .map(|d| Path::new(d).join(tool))
                .find(|p| p.exists())
                .unwrap_or_else(|| panic!("{tool} not found"));
            let link = path.join(tool);
            if !link.exists() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(found, link).unwrap();
            }
        }
        let docker = path.join("docker");
        std::fs::write(&docker, FAKE_DOCKER).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let nearbee = f.path("nearbee-bin");
        std::fs::write(&nearbee, "host nearbee").unwrap();
        std::fs::create_dir_all(f.path("cfg")).unwrap();
        std::fs::create_dir_all(f.path("run")).unwrap();
        Self { f, path }
    }

    fn sha(&self, p: &str) -> String {
        let out = Command::new("sha256sum")
            .arg(self.f.path(p))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    }

    /// The fake image carries exactly the host binaries.
    fn matching(&self) -> Vec<(String, String)> {
        vec![
            ("FAKE_APB_SHA".into(), self.sha("apb")),
            ("FAKE_NEARBEE_SHA".into(), self.sha("nearbee-bin")),
        ]
    }

    fn cmd(&self, script: &str, args: &[&str], extra: &[(String, String)]) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts")
            .join(script);
        Command::new(self.path.join("bash"))
            .arg(script)
            .args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.f.dir.path())
            .env("XDG_RUNTIME_DIR", self.f.path("run"))
            .env("APB_BIN", self.f.path("apb"))
            .env("NEARBEE_BIN", self.f.path("nearbee-bin"))
            .env("NEARBEE_ENV_FILE", self.f.path("cfg/.env"))
            .env("NEARBEE_APB_CONFIG_DIR", self.f.path("config"))
            .env("NEARBEE_APB_WORKSPACE", self.f.path("ws"))
            .env("NEARBEE_APB_SANDBOX", "1")
            .env("FAKE_DOCKER_LOG", self.f.path("docker.log"))
            .env("FAKE_SYSTEMD_LOG", self.f.path("systemd.log"))
            .envs(extra.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .output()
            .unwrap()
    }

    /// Adds a fake systemd (a `systemd-run` that logs its arguments, a
    /// `systemctl` with a user session and no active unit) and a `curl`
    /// that answers 200, so `start` runs to its end.
    fn with_systemd(&self) {
        for (name, body) in [
            (
                "systemd-run",
                "#!/bin/bash\nprintf '%s\\n' \"$@\" > \"$FAKE_SYSTEMD_LOG\"\n",
            ),
            (
                "systemctl",
                "#!/bin/bash\ncase \"$*\" in *is-active*) exit 3 ;; esac\nexit 0\n",
            ),
            ("curl", "#!/bin/bash\nprintf 200\n"),
        ] {
            let p = self.path.join(name);
            std::fs::write(&p, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        for tool in ["seq", "sleep"] {
            let found = ["/usr/bin", "/bin"]
                .iter()
                .map(|d| Path::new(d).join(tool))
                .find(|p| p.exists())
                .unwrap_or_else(|| panic!("{tool} not found"));
            let link = self.path.join(tool);
            if !link.exists() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(found, link).unwrap();
            }
        }
        std::fs::write(self.f.path("cfg/.env"), "NEARAI_API_KEY=synthetic-key\n").unwrap();
    }

    /// The arguments the fake `systemd-run` got, one per line.
    fn unit_args(&self) -> Vec<String> {
        std::fs::read_to_string(self.f.path("systemd.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn start(&self, extra: &[(String, String)]) -> Output {
        self.cmd("apb-instance.sh", &["start"], extra)
    }

    fn docker_log(&self) -> String {
        std::fs::read_to_string(self.f.path("docker.log")).unwrap_or_default()
    }
}

fn kv(k: &str, v: &str) -> (String, String) {
    (k.into(), v.into())
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The server key never goes into the sandbox (spec apb-sandbox, API key).
#[cfg(unix)]
#[test]
fn sandbox_start_refuses_the_api_key_in_the_passed_variables() {
    let s = Sandbox::new();
    let mut env = s.matching();
    env.push(kv("NEARBEE_APB_ENV_VARS", "NEARAI_API_KEY APB_API_KEY"));
    let out = s.start(&env);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("APB_API_KEY"),
        "{}",
        stderr_of(&out)
    );
    assert!(!s.docker_log().contains("run "), "{}", s.docker_log());
}

/// An image built from other binaries is refused, naming the rebuild
/// (spec apb-sandbox, Updated host binary).
#[cfg(unix)]
#[test]
fn sandbox_start_refuses_an_image_with_another_nearbee() {
    let s = Sandbox::new();
    let env = [
        kv("FAKE_APB_SHA", &s.sha("apb")),
        kv("FAKE_NEARBEE_SHA", "0000"),
    ];
    let out = s.start(&env);
    assert!(!out.status.success());
    let err = stderr_of(&out);
    assert!(
        err.contains("nearbee") && err.contains("sandbox/build.sh"),
        "{err}"
    );
    assert!(!s.f.path("run/nearbee-apb.env").exists());
}

/// OpenCode configuration planted in the sandbox home is refused.
#[cfg(unix)]
#[test]
fn sandbox_start_refuses_opencode_entries_in_the_sandbox_home() {
    let s = Sandbox::new();
    std::fs::create_dir_all(s.f.path(".local/share/nearbee/sandbox-home/.opencode/plugin"))
        .unwrap();
    let out = s.start(&s.matching());
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains(".opencode holds entries"),
        "{}",
        stderr_of(&out)
    );
}

/// nearbee.yaml next to the .env must name the mounted workspace.
#[cfg(unix)]
#[test]
fn sandbox_start_refuses_a_config_naming_another_workspace() {
    let s = Sandbox::new();
    std::fs::write(
        s.f.path("cfg/nearbee.yaml"),
        "apb:\n  url: http://127.0.0.1:7322\n  workspace_path: ./elsewhere\n",
    )
    .unwrap();
    let out = s.start(&s.matching());
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("names the workspace"),
        "{}",
        stderr_of(&out)
    );
}

/// With no key the start issues one, writes it to Nearbee's .env as
/// APB_API_KEY and never prints it.
#[cfg(unix)]
#[test]
fn sandbox_start_issues_a_key_into_the_env_file_without_printing_it() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    std::fs::write(s.f.path("cfg/.env"), "NEARAI_API_KEY=synthetic-key\n").unwrap();
    let out = s.start(&s.matching());
    // No systemd in the test PATH: the start stops after the key step.
    assert!(stderr_of(&out).contains("systemd"), "{}", stderr_of(&out));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        stderr_of(&out)
    );
    assert!(
        !all.contains("apb_fakeissuedkey"),
        "the key was printed: {all}"
    );
    let env = std::fs::read_to_string(s.f.path("cfg/.env")).unwrap();
    assert_eq!(
        env,
        "NEARAI_API_KEY=synthetic-key\nAPB_API_KEY=apb_fakeissuedkey0123456789\n"
    );
    let mode = std::fs::metadata(s.f.path("cfg/.env"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
    assert_eq!(s.docker_log().matches("server key issue").count(), 1);
}

/// With a key already issued, no new key is issued; without APB_API_KEY in
/// the .env the start stops and says why (spec apb-sandbox, Second start).
#[cfg(unix)]
#[test]
fn sandbox_start_issues_no_second_key() {
    let s = Sandbox::new();
    let mut env = s.matching();
    env.push(kv("FAKE_KEY_LISTED", "1"));
    std::fs::write(s.f.path("cfg/.env"), "APB_API_KEY=apb_stored\n").unwrap();
    let out = s.start(&env);
    assert!(stderr_of(&out).contains("systemd"), "{}", stderr_of(&out));
    assert!(
        !s.docker_log().contains("server key issue"),
        "{}",
        s.docker_log()
    );
    std::fs::write(s.f.path("cfg/.env"), "NEARAI_API_KEY=x\n").unwrap();
    let out = s.start(&env);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("has no APB_API_KEY"),
        "{}",
        stderr_of(&out)
    );
    assert!(!s.docker_log().contains("server key issue"));
}

/// The wrapper runs the apb CLI in the running dashboard container from
/// the caller's directory when it is mounted, bounded by a timeout inside
/// the container (spec apb-sandbox, apb CLI in the sandbox).
#[cfg(unix)]
#[test]
fn the_wrapper_runs_apb_in_the_running_container_from_a_mounted_directory() {
    let s = Sandbox::new();
    let ws = s.f.path("ws");
    let cfg = s.f.path("config");
    std::fs::create_dir_all(&cfg).unwrap();
    let env = [
        kv("FAKE_RUNNING", "true"),
        kv("FAKE_CONFIG_DIR", &cfg.display().to_string()),
        kv("APB_CONFIG_DIR", &cfg.display().to_string()),
    ];
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-sandboxed");
    let out = Command::new(s.path.join("bash"))
        .arg(script)
        .args(["list"])
        .current_dir(&ws)
        .env_clear()
        .env("PATH", &s.path)
        .env("HOME", s.f.dir.path())
        .env("XDG_RUNTIME_DIR", s.f.path("run"))
        .env("APB_BIN", "/abs/scripts/apb-sandboxed")
        .env("NEARBEE_APB_WORKSPACE", &ws)
        .env("FAKE_DOCKER_LOG", s.f.path("docker.log"))
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr_of(&out));
    let log = s.docker_log();
    let exec = log.lines().find(|l| l.starts_with("exec ")).expect(&log);
    assert!(exec.contains(&format!("-w {}", ws.display())), "{exec}");
    assert!(
        exec.contains(&format!("APB_CONFIG_DIR={}", cfg.display())),
        "{exec}"
    );
    assert!(
        exec.contains("timeout --kill-after=5 25 apb list"),
        "{exec}"
    );
}

/// While the dashboard is down the wrapper takes a one-off container with
/// no network.
#[cfg(unix)]
#[test]
fn the_wrapper_uses_a_one_off_container_without_network_when_the_dashboard_is_down() {
    let s = Sandbox::new();
    let out = s.cmd("apb-sandboxed", &["list"], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let log = s.docker_log();
    let run = log.lines().find(|l| l.starts_with("run ")).expect(&log);
    assert!(run.contains("--network none"), "{run}");
    assert!(
        run.contains("--read-only") && run.contains("--cap-drop ALL"),
        "{run}"
    );
    assert!(run.contains("timeout --kill-after=5 25 apb list"), "{run}");
}

/// `down` removes the dashboard and gateway containers and both networks.
#[cfg(unix)]
#[test]
fn sandbox_down_removes_the_containers_and_networks() {
    let s = Sandbox::new();
    let out = s.cmd("apb-instance.sh", &["down"], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let log = s.docker_log();
    for want in [
        "rm -f nearbee-apb-sbx",
        "rm -f nearbee-apb-gw",
        "network rm nearbee-sbx-int",
        "network rm nearbee-sbx-out",
    ] {
        assert!(log.contains(want), "{want} missing: {log}");
    }
}

/// A running sandbox that serves another config directory is never used
/// for the caller's APB_CONFIG_DIR.
#[cfg(unix)]
#[test]
fn the_wrapper_refuses_a_running_sandbox_of_another_config_directory() {
    let s = Sandbox::new();
    std::fs::create_dir_all(s.f.path("config")).unwrap();
    let env = [
        kv("FAKE_RUNNING", "true"),
        kv("FAKE_CONFIG_DIR", "/elsewhere/apb-config"),
    ];
    let out = s.cmd("apb-sandboxed", &["list"], &env);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("/elsewhere/apb-config"),
        "{}",
        stderr_of(&out)
    );
    assert!(!s.docker_log().contains("exec "), "{}", s.docker_log());
}

// Review fixes (change apb-sandbox, code review round one): host-side code
// never follows a symlink or writes through a predictable path in the
// workspace, the config directory or the sandbox home.

/// A start with the fake systemd reaches its end: the unit is started.
#[cfg(unix)]
#[test]
fn sandbox_start_with_systemd_starts_the_unit() {
    let s = Sandbox::new();
    s.with_systemd();
    let out = s.start(&s.matching());
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(
        s.unit_args().iter().any(|a| a == "apb"),
        "{:?}",
        s.unit_args()
    );
}

/// Finding 1: policy.yaml is always bound read-only; a start creates it
/// with apb's defaults when it is missing, so a step can never write the
/// one the next start protects.
#[cfg(unix)]
#[test]
fn sandbox_start_creates_policy_yaml_and_binds_it_read_only() {
    let s = Sandbox::new();
    s.with_systemd();
    let out = s.start(&s.matching());
    assert!(out.status.success(), "{}", stderr_of(&out));
    let cfg = s.f.path("config").canonicalize().unwrap();
    let policy = cfg.join("policy.yaml");
    let text = std::fs::read_to_string(&policy).unwrap();
    let v: serde_json::Value = yaml_serde::from_str(&text).unwrap();
    // It sets nothing: apb's own defaults apply.
    assert_eq!(v, serde_json::json!({}), "{text}");
    let bind = format!("type=bind,src={p},dst={p},readonly", p = policy.display());
    assert!(s.unit_args().contains(&bind), "{:?}", s.unit_args());
}

/// Finding 1: a protected config file that is a symlink (to a host file
/// the container would then see) is refused before anything starts.
#[cfg(unix)]
#[test]
fn sandbox_start_refuses_a_symlinked_protected_config_file() {
    for name in ["policy.yaml", "server-auth.yaml", "run-origin.key"] {
        let s = Sandbox::new();
        s.with_systemd();
        let secret = s.f.path("host-secret.yml");
        std::fs::write(&secret, "token: host-only\n").unwrap();
        std::fs::create_dir_all(s.f.path("config")).unwrap();
        std::os::unix::fs::symlink(&secret, s.f.path("config").join(name)).unwrap();
        let out = s.start(&s.matching());
        assert!(!out.status.success(), "{name}: started");
        assert!(
            stderr_of(&out).contains(name) && stderr_of(&out).contains("symlink"),
            "{name}: {}",
            stderr_of(&out)
        );
        assert!(s.unit_args().is_empty(), "{name}: unit started");
        assert!(
            !s.docker_log().contains("host-secret"),
            "{name}: {}",
            s.docker_log()
        );
    }
}

/// Finding 4: the internal network gives the host no address on its
/// bridge; an existing one with another subnet or a host address is made
/// again.
#[cfg(unix)]
#[test]
fn sandbox_start_makes_the_internal_network_without_a_host_address() {
    let s = Sandbox::new();
    s.with_systemd();
    let out = s.start(&s.matching());
    assert!(out.status.success(), "{}", stderr_of(&out));
    let log = s.docker_log();
    let create = log
        .lines()
        .find(|l| l.starts_with("network create") && l.contains("--internal"))
        .expect(&log);
    assert!(
        create.contains("-o com.docker.network.bridge.gateway_mode_ipv4=isolated"),
        "{create}"
    );
    for existing in ["172.30.77.0/24|nat", "10.9.9.0/24|isolated"] {
        let s = Sandbox::new();
        s.with_systemd();
        let mut env = s.matching();
        env.push(kv("FAKE_NET_INT", existing));
        let out = s.start(&env);
        assert!(out.status.success(), "{existing}: {}", stderr_of(&out));
        let log = s.docker_log();
        assert!(
            log.contains("network rm nearbee-sbx-int"),
            "{existing}: {log}"
        );
        assert!(log.contains("--internal"), "{existing}: {log}");
    }
    let s = Sandbox::new();
    s.with_systemd();
    let mut env = s.matching();
    env.push(kv("FAKE_NET_INT", "172.30.77.0/24|isolated"));
    assert!(s.start(&env).status.success());
    assert!(!s.docker_log().contains("network rm"), "{}", s.docker_log());
}

/// Finding 5: a gateway whose attach failed is removed, and one that is
/// not attached at its address is never reused.
#[cfg(unix)]
#[test]
fn sandbox_start_leaves_no_half_made_gateway() {
    let s = Sandbox::new();
    s.with_systemd();
    let mut env = s.matching();
    env.push(kv("FAKE_CONNECT_FAIL", "1"));
    let out = s.start(&env);
    assert!(!out.status.success());
    let log = s.docker_log();
    let connect = log.find("network connect").expect(&log);
    assert!(log[connect..].contains("rm -f nearbee-apb-gw"), "{log}");

    let s = Sandbox::new();
    s.with_systemd();
    // First start: learn the hash the gateway gets.
    assert!(s.start(&s.matching()).status.success());
    let hash = s
        .docker_log()
        .lines()
        .find(|l| l.starts_with("create "))
        .and_then(|l| l.split("nearbee.gateway-hash=").nth(1))
        .map(|r| r.split(' ').next().unwrap().to_owned())
        .expect("a gateway create");
    std::fs::remove_file(s.f.path("docker.log")).unwrap();
    let mut env = s.matching();
    env.push(kv("FAKE_GW_HASH", &hash));
    env.push(kv("FAKE_GW_IP", ""));
    assert!(s.start(&env).status.success());
    assert!(
        s.docker_log().contains("rm -f nearbee-apb-gw"),
        "{}",
        s.docker_log()
    );
    std::fs::remove_file(s.f.path("docker.log")).unwrap();
    env.pop();
    env.push(kv("FAKE_GW_IP", "172.30.77.2"));
    assert!(s.start(&env).status.success());
    assert!(
        !s.docker_log().contains("rm -f nearbee-apb-gw"),
        "{}",
        s.docker_log()
    );
}

/// Finding 5: a start that fails after writing the key file removes it.
#[cfg(unix)]
#[test]
fn a_failed_sandbox_start_leaves_no_key_file() {
    let s = Sandbox::new();
    std::fs::write(s.f.path("cfg/.env"), "NEARAI_API_KEY=synthetic-key\n").unwrap();
    // No systemd in the PATH: the start fails after the key step.
    let out = s.start(&s.matching());
    assert!(!out.status.success());
    assert!(!s.f.path("run/nearbee-apb.env").exists());
    let s = Sandbox::new();
    s.with_systemd();
    let mut env = s.matching();
    env.push(kv("FAKE_CONNECT_FAIL", "1"));
    assert!(!s.start(&env).status.success());
    assert!(!s.f.path("run/nearbee-apb.env").exists());
}

/// Finding 6: no passed variable may shadow a value the unit and the
/// container set, nor steer the docker client.
#[cfg(unix)]
#[test]
fn sandbox_start_refuses_passed_names_that_collide_with_the_sandbox_environment() {
    for name in [
        "HOME",
        "PATH",
        "XDG_CONFIG_HOME",
        "APB_CONFIG_DIR",
        "NEARBEE_BIN",
        "DOCKER_CONFIG",
        "DOCKER_HOST",
        "GIT_AUTHOR_EMAIL",
    ] {
        let s = Sandbox::new();
        let mut env = s.matching();
        env.push(kv(
            "NEARBEE_APB_ENV_VARS",
            &format!("NEARAI_API_KEY {name}"),
        ));
        let out = s.start(&env);
        assert!(!out.status.success(), "{name}");
        assert!(
            stderr_of(&out).contains(name),
            "{name}: {}",
            stderr_of(&out)
        );
        assert!(!s.docker_log().contains("run "), "{name}");
    }
}

/// Finding 7: list settings are never globbed, and a mount path that
/// could add mount options is refused.
#[cfg(unix)]
#[test]
fn sandbox_lists_are_not_globbed_and_mount_paths_cannot_add_options() {
    let s = Sandbox::new();
    s.with_systemd();
    let cwd = s.f.path("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(cwd.join("evil.example"), "").unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/apb-instance.sh");
    let mut env = s.matching();
    env.push(kv("NEARBEE_SANDBOX_ALLOW", "*"));
    let out = Command::new(s.path.join("bash"))
        .arg(&script)
        .arg("start")
        .current_dir(&cwd)
        .env_clear()
        .env("PATH", &s.path)
        .env("HOME", s.f.dir.path())
        .env("XDG_RUNTIME_DIR", s.f.path("run"))
        .env("APB_BIN", s.f.path("apb"))
        .env("NEARBEE_BIN", s.f.path("nearbee-bin"))
        .env("NEARBEE_ENV_FILE", s.f.path("cfg/.env"))
        .env("NEARBEE_APB_CONFIG_DIR", s.f.path("config"))
        .env("NEARBEE_APB_WORKSPACE", s.f.path("ws"))
        .env("NEARBEE_APB_SANDBOX", "1")
        .env("FAKE_DOCKER_LOG", s.f.path("docker.log"))
        .env("FAKE_SYSTEMD_LOG", s.f.path("systemd.log"))
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .output()
        .unwrap();
    assert!(!out.status.success(), "a glob passed as an allowed host");
    assert!(
        stderr_of(&out).contains("not a host name in the allowlist: *"),
        "{}",
        stderr_of(&out)
    );

    let s = Sandbox::new();
    s.with_systemd();
    let odd = s.f.path("ro,dst=/etc");
    std::fs::create_dir_all(&odd).unwrap();
    let mut env = s.matching();
    env.push(kv("NEARBEE_SANDBOX_MOUNTS_RO", &odd.display().to_string()));
    let out = s.start(&env);
    assert!(!out.status.success(), "a mount path with a comma passed");
    assert!(stderr_of(&out).contains("mount"), "{}", stderr_of(&out));
    assert!(s.unit_args().is_empty());
}

/// Finding 8: the gateway runs unprivileged with every capability
/// dropped, no new privileges and a read-only root.
#[cfg(unix)]
#[test]
fn sandbox_gateway_runs_hardened() {
    let s = Sandbox::new();
    s.with_systemd();
    assert!(s.start(&s.matching()).status.success());
    let log = s.docker_log();
    let create = log
        .lines()
        .find(|l| l.starts_with("create ") && l.contains("nearbee-apb-gw"))
        .expect(&log);
    assert!(!create.contains("--cap-add"), "{create}");
    for want in [
        "--user 101:101",
        "--cap-drop ALL",
        "--security-opt no-new-privileges",
        "--read-only",
        "--tmpfs /var/cache/nginx",
    ] {
        assert!(create.contains(want), "{want} missing: {create}");
    }
}

/// Finding 2: in sandbox mode `install` writes the workspace from inside
/// the container, never on the host, so a symlink a step planted in the
/// workspace cannot redirect a host write; the bundles go on stdin.
#[cfg(unix)]
#[test]
fn sandbox_install_writes_the_workspace_inside_the_container() {
    let s = Sandbox::new();
    std::fs::create_dir_all(s.f.path("ws/.apb")).unwrap();
    std::fs::create_dir_all(s.f.path("config")).unwrap();
    let target = s.f.path("bashrc");
    std::fs::write(&target, "original\n").unwrap();
    std::fs::write(s.f.path("ws/.apb/config.yaml"), "touch /tmp/pwned\n").unwrap();
    std::os::unix::fs::symlink(&target, s.f.path("ws/.apb/config.yaml.nearbee-tmp")).unwrap();
    std::os::unix::fs::symlink(&target, s.f.path("ws/opencode.json")).unwrap();
    let ws = s.f.path("ws").display().to_string();
    let out = s.cmd("apb-instance.sh", &["install", &ws], &[]);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original\n");
    assert!(
        !s.f.path("ws/.apb/profiles").exists(),
        "profiles written on the host"
    );
    let log = s.docker_log();
    assert!(
        log.lines().any(|l| l.contains("python3 -c")),
        "no container write: {log}"
    );
    assert!(log.contains("import /dev/stdin"), "{}", stderr_of(&out));
    // Only the default policy.yaml a sandbox command creates.
    let left: Vec<_> = std::fs::read_dir(s.f.path("config"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n != "policy.yaml")
        .collect();
    assert!(left.is_empty(), "files in the config directory: {left:?}");
}

/// Finding 2 (host mode too): the workspace writes never follow a
/// symlink, neither a planted file nor a planted directory.
#[cfg(unix)]
#[test]
fn install_never_writes_through_a_symlink_in_the_workspace() {
    let f = Fake::new();
    let target = f.path("bashrc");
    std::fs::write(&target, "original\n").unwrap();
    std::os::unix::fs::symlink(&target, f.path("ws/opencode.json")).unwrap();
    std::os::unix::fs::symlink(&target, f.path("ws/.apb/config.yaml.nearbee-tmp")).unwrap();
    assert_ok(&f.install("digest", "new"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original\n");
    let meta = std::fs::symlink_metadata(f.path("ws/opencode.json")).unwrap();
    assert!(meta.file_type().is_file(), "opencode.json is still a link");

    let f = Fake::new();
    let outside = f.path("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, f.path("ws/.apb/profiles")).unwrap();
    let out = f.install("digest", "new");
    assert!(!out.status.success(), "wrote through a symlinked directory");
    assert!(stderr_of(&out).contains("symlink"), "{}", stderr_of(&out));
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);

    let f = Fake::new();
    std::fs::write(&target, "decisions:\n").unwrap();
    std::os::unix::fs::symlink(&target, f.path("ws/.apb/config.yaml")).unwrap();
    let out = f.install("digest", "new");
    assert!(!out.status.success(), "rewrote a symlinked config.yaml");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "decisions:\n");
}

/// The same rule for `register`: in sandbox mode the directory and its git
/// repository are made inside the container.
#[cfg(unix)]
#[test]
fn sandbox_register_runs_git_inside_the_container() {
    let s = Sandbox::new();
    std::fs::create_dir_all(s.f.path("ws")).unwrap();
    let dir = s.f.path("ws/project").display().to_string();
    let _ = s.cmd("apb-instance.sh", &["register", &dir, "--git"], &[]);
    let log = s.docker_log();
    assert!(
        log.lines()
            .any(|l| l.contains("git -C") && l.contains("init")),
        "{log}"
    );
    assert!(!s.f.path("ws/project/.git").exists(), "git ran on the host");
}
