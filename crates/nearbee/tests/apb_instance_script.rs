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
echo "$*" >> "$FAKE_LOG"
state="$(dirname "$FAKE_LOG")/state"; mkdir -p "$state"
bundle_field() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1" "$2"; }
case "$1" in
  import)
    file="$2"; id="$(bundle_field "$file" id)"; ver="$(bundle_field "$file" version)"
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
      *"--accept sha256:aaaa"*) touch "$state/$id.imported"; exit 0 ;;
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
fn install_imports_and_approves_every_playbook_by_the_digests_apb_printed() {
    let f = Fake::new();
    let stdout = assert_ok(&f.install("digest", "new"));
    assert_reported_approved(&stdout);
    let log = f.log();
    for id in smoke_ids() {
        let ver = smoke_version(&id);
        let imports = calls(&log, "import ", &id);
        assert_eq!(
            imports
                .iter()
                .filter(|l| l.ends_with("--accept sha256:aaaa"))
                .count(),
            1,
            "{id}: one import accepting the printed bundle digest: {imports:?}"
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
