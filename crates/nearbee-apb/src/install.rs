//! Installing a playbook into Nearbee's apb instance as a bundle, through
//! the `apb` CLI (D-004 allows the CLI where REST has no route): the logic
//! of `scripts/apb-instance.sh install`, moved into Rust so publishing can
//! drive it.
//!
//! Safety rules carried over from the script:
//!
//! - the preview before an import must say what the bundle changes; the
//!   same version with other content is refused, never overwritten;
//! - only the content just built is approved: the workspace must hold the
//!   bundle as its current version before the approval and still after it,
//!   otherwise the approval is revoked;
//! - the installed current version is read back and must carry
//!   `trial_candidates: off` before any market call happens.

use std::path::PathBuf;
use std::process::Stdio;

use serde_json::{Value, json};

/// One script of the playbook bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub name: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// The CLI of one apb instance: a binary, a workspace directory and the
/// instance's config directory.
#[derive(Debug, Clone)]
pub struct ApbCli {
    pub bin: PathBuf,
    pub workspace: PathBuf,
    pub config_dir: PathBuf,
}

/// An install failure. Output of the apb CLI is quoted into the message.
#[derive(Debug, thiserror::Error)]
#[error("{step}: {detail}")]
pub struct InstallError {
    pub step: &'static str,
    pub detail: String,
}

/// What was installed and read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub playbook_id: String,
    pub version: String,
    /// SHA-256 (lowercase hex) of the materialised playbook text.
    pub sha256: String,
}

/// Reads a child's pipe to the end on its own thread, so the child can
/// never block on a full pipe buffer while the parent waits for its exit
/// (N10).
fn drain<R: std::io::Read + Send + 'static>(mut p: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = p.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}

impl ApbCli {
    /// Runs the CLI in the workspace; on failure the error names the step
    /// and carries the (already redacted-by-apb) output.
    /// How long one apb call may take: a prompting or hung apb must not
    /// block a publish forever.
    const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

    fn run(&self, step: &'static str, args: &[&str]) -> Result<String, InstallError> {
        // stdin is null, so a prompting apb reads EOF instead of blocking.
        let mut child = std::process::Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .current_dir(&self.workspace)
            .env("APB_CONFIG_DIR", &self.config_dir)
            .env_remove("NO_COLOR")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| InstallError {
                step,
                detail: format!("cannot run {}: {e}", self.bin.display()),
            })?;
        let deadline = std::time::Instant::now() + Self::CLI_TIMEOUT;
        // The pipes are drained on threads while the child runs, so a big
        // answer can never block the child on a full pipe buffer (N10).
        let stdout_reader = drain(child.stdout.take());
        let stderr_reader = drain(child.stderr.take());
        let status = loop {
            match child.try_wait().map_err(|e| InstallError {
                step,
                detail: format!("{}: {e}", self.bin.display()),
            })? {
                Some(st) => break st,
                None => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(InstallError {
                            step,
                            detail: format!(
                                "`apb {}` did not finish within {} s and was killed",
                                args.first().unwrap_or(&""),
                                Self::CLI_TIMEOUT.as_secs()
                            ),
                        });
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        };
        // The child is done: the drained output is complete.
        let stdout = stdout_reader.join().map_err(|_| InstallError {
            step,
            detail: "the output reader failed".into(),
        })?;
        let stderr = stderr_reader.join().map_err(|_| InstallError {
            step,
            detail: "the output reader failed".into(),
        })?;
        let out = std::process::Output {
            status,
            stdout,
            stderr,
        };
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        if !out.status.success() {
            return Err(InstallError {
                step,
                detail: format!(
                    "`apb {}` failed ({}): {}",
                    args.first().unwrap_or(&""),
                    out.status.code().unwrap_or(-1),
                    text.trim()
                ),
            });
        }
        Ok(text)
    }
}

/// Builds the materialised playbook text: `id: <handle>`, profile names
/// unchanged, `supervisor.policy.trial_candidates: off` set, version
/// derived from the digest of the result.
pub fn materialise(source: &str, handle: &str) -> Result<String, InstallError> {
    if !handle
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || handle.is_empty()
    {
        return Err(InstallError {
            step: "materialise",
            detail: format!("`{handle}` is not a safe playbook id"),
        });
    }
    // The source must not set trial_candidates to anything but off. The
    // check is structural (parsed YAML), so comments and flow style cannot
    // fool it.
    if trial_candidates_off(source) == Some(false) {
        return Err(InstallError {
            step: "materialise",
            detail:
                "the source playbook sets `trial_candidates` to something other than `off`; publishing allows only `trial_candidates: off`"
                    .into(),
        });
    }
    let mut text = String::with_capacity(source.len() + 128);
    let mut in_supervisor = false;
    let mut supervisor_written = false;
    let mut id_set = false;
    let mut version_slot = false;
    // A dropped `policy:` line takes its whole block with it (O-13):
    // child lines deeper than the dropped header are skipped until a
    // line at or above the header's indentation ends the block.
    let mut drop_block_indent: Option<usize> = None;
    for line in source.lines() {
        let top = !line.starts_with(' ');
        let t = line.trim_start();
        let indent = line.len() - t.len();
        if let Some(limit) = drop_block_indent {
            // Blank and comment-only lines never end the dropped block
            // (L2): a blank line has indent 0 and would otherwise let a
            // deeper child after it merge back into the output.
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if indent > limit {
                continue;
            }
            drop_block_indent = None;
        }
        if top && t.starts_with("supervisor:") {
            in_supervisor = true;
            supervisor_written = true;
            text.push_str(line);
            text.push('\n');
            text.push_str("  policy:\n    trial_candidates: off\n");
            continue;
        }
        if top {
            in_supervisor = false;
            if t.starts_with("id:") {
                id_set = true;
                text.push_str(&format!("id: {handle}\n"));
                continue;
            }
            if t.starts_with("version:") {
                version_slot = true;
                text.push_str("__NEARBEE_VERSION__\n");
                continue;
            }
        }
        if in_supervisor && t.starts_with("policy:") {
            // Replaced by the adapter's own policy block; the source
            // block, its header and all of its children, is dropped.
            drop_block_indent = Some(indent);
            continue;
        }
        if in_supervisor && t.starts_with("trial_candidates:") {
            // Already written by the block above.
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    if !id_set {
        return Err(InstallError {
            step: "materialise",
            detail: "the source playbook has no `id:` line".into(),
        });
    }
    if !version_slot {
        // No version line in the source: insert one after the id.
        let after = text
            .find(&format!("id: {handle}\n"))
            .map(|i| i + format!("id: {handle}\n").len())
            .unwrap_or(0);
        text.insert_str(after, "__NEARBEE_VERSION__\n");
    }
    let version = derived_version(&text);
    let mut text = text.replace("__NEARBEE_VERSION__", &format!("version: {version}"));
    if !supervisor_written {
        // Append the supervisor block before the first `nodes:` line if it
        // exists, else at the end.
        let insert_at = text.find("\nnodes:\n").map(|i| i + 1).unwrap_or(text.len());
        text.insert_str(
            insert_at,
            "supervisor:\n  policy:\n    trial_candidates: off\n",
        );
    }
    Ok(text)
}

/// Whether the playbook text sets `supervisor.policy.trial_candidates`
/// to `off`, parsed structurally. Only that exact path counts (O-13): a
/// `trial_candidates` key anywhere else is not the trial setting. `None`
/// when the key is absent.
fn trial_candidates_off(text: &str) -> Option<bool> {
    let v: serde_json::Value = yaml_serde::from_str(text).ok()?;
    v.get("supervisor")?
        .get("policy")?
        .get("trial_candidates")?
        .as_str()
        .map(|s| s.trim() == "off")
}

/// The `version:` line of a materialised text.
fn version_in(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("version: ").map(str::trim))
        .map(str::to_owned)
}

/// The version derived from the materialised text: `1.0.<8 hex>` of the
/// SHA-256 of the text without its `version:` line, so an unchanged source
/// keeps its version and any change gets a new one.
pub fn derived_version(text_without_version: &str) -> String {
    let hex = nearbee_core::journal::sha256_hex(text_without_version.as_bytes());
    format!("1.0.{}", &hex[..8])
}

fn bundle_id_and_version(text: &str) -> Option<(String, String)> {
    let id = text
        .lines()
        .find_map(|l| l.strip_prefix("id: ").map(str::trim))?
        .to_owned();
    Some((id, version_in(text)?))
}

/// Builds the apb bundle JSON for the text and its scripts.
pub fn build_bundle(text: &str, scripts: &[Script]) -> Value {
    json!({
        "apb_bundle": 2,
        "id": bundle_id_and_version(text).map(|(id, _)| id).unwrap_or_default(),
        "version": bundle_id_and_version(text).map(|(_, v)| v).unwrap_or_default(),
        "playbook": text,
        "scripts": scripts.iter().map(|s| json!({
            "path": s.name,
            "content_base64": base64_encode(&s.bytes),
            "executable": s.executable,
        })).collect::<Vec<_>>(),
    })
}

fn base64_encode(bytes: &[u8]) -> String {
    // Base64 without external crates: small alphabets are easy to get
    // wrong, so the alphabet and padding follow RFC 4648 exactly.
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

const UNCHANGED_PREFIX: &str = "unchanged: the current version already holds this bundle";

fn extract_digest(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|l| {
            let i = l.find("--accept sha256:")?;
            let rest = &l[i + "--accept ".len()..];
            let digest = rest.split_whitespace().next()?;
            digest.starts_with("sha256:").then(|| digest.to_owned())
        })
        .or_else(|| {
            output.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("sha256:")
                    .map(|d| format!("sha256:{d}"))
            })
        })
}

/// What a read-back of the installed playbook found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBack {
    pub version: String,
    pub trial_off: bool,
}

/// Installs `materialised` (with its scripts) into the workspace and
/// approves exactly that bundle, then reads the installed current version
/// back through `apb export` and refuses one without
/// `trial_candidates: off` or with a version other than the one installed.
pub fn install_playbook(
    cli: &ApbCli,
    materialised: &str,
    scripts: &[Script],
) -> Result<Installed, InstallError> {
    let (id, version) = bundle_id_and_version(materialised).ok_or_else(|| InstallError {
        step: "bundle",
        detail: "the materialised text has no id or version".into(),
    })?;
    let bundle = build_bundle(materialised, scripts);
    // A private temp directory (random name, 0700), removed on drop: no
    // predictable path in the shared temp dir for the bundle to be swapped.
    let tmp = tempfile::tempdir().map_err(|e| InstallError {
        step: "bundle",
        detail: e.to_string(),
    })?;
    let bundle_path = tmp.path().join(format!("{id}.json"));
    std::fs::write(
        &bundle_path,
        serde_json::to_vec(&bundle).expect("bundle serializes"),
    )
    .map_err(|e| InstallError {
        step: "bundle",
        detail: e.to_string(),
    })?;
    // The digest is derived from the bundle just built, never from apb's
    // free-form output.
    let bundle_digest = format!(
        "sha256:{}",
        nearbee_core::journal::sha256_hex(&serde_json::to_vec(&bundle).expect("bundle serializes"))
    );
    let bundle_arg = bundle_path.display().to_string();
    // `tmp` lives to the end of the function, so every apb call below sees
    // the bundle at this path.

    let holds_bundle = |cli: &ApbCli| -> Result<bool, InstallError> {
        let out = cli.run("preview", &["import", &bundle_arg, "--preview"])?;
        Ok(out.contains(&format!("bundle {id} @ {version}: {UNCHANGED_PREFIX}")))
    };

    // 1. Import the bundle. Without a terminal, apb prints the preview and
    //    exits without writing unless the call names the bundle digest.
    let _imported: String = match cli.run("import", &["import", &bundle_arg]) {
        Ok(text) if text.contains("nothing to import") || text.contains(UNCHANGED_PREFIX) => {
            // apb compared the bundle with its current version: same
            // definition and same scripts.
            text
        }
        Ok(text) => text,
        Err(e) if e.detail.contains("already exists") => {
            return Err(InstallError {
                step: "import",
                detail: format!(
                    "{id} {version} exists in the workspace with other content; bump the version in the source playbook. {e}"
                ),
            });
        }
        Err(e) => {
            // The acceptance names the digest of the bundle we just built,
            // never text apb happened to print.
            let _ = e;
            let accept_arg = bundle_digest.as_str();
            cli.run("accept", &["import", &bundle_arg, "--accept", accept_arg])?;
            String::new()
        }
    };
    // 2. The workspace must hold exactly this bundle before the approval.
    if !holds_bundle(cli)? {
        return Err(InstallError {
            step: "verify",
            detail: format!(
                "{id} {version} in the workspace is not the bundle just built; nothing approved"
            ),
        });
    }
    // 3. Approve the version; apb answers the trust digest on exit 2.
    let mut approved_via_digest = false;
    match cli.run(
        "trust approve",
        &["trust", "approve", &id, "--version", &version],
    ) {
        Ok(_) => {}
        Err(approve_error) => {
            let digest = extract_digest(&approve_error.detail).ok_or_else(|| InstallError {
                step: "trust approve",
                detail: format!(
                    "{id} {version} not approved: apb printed no trust digest. {approve_error}"
                ),
            })?;
            let digest_arg = digest.as_str();
            cli.run(
                "trust approve digest",
                &[
                    "trust",
                    "approve",
                    &id,
                    "--version",
                    &version,
                    "--digest",
                    digest_arg,
                ],
            )?;
            approved_via_digest = true;
        }
    }
    // 4. Still the same bundle after the approval, else revoke.
    if !holds_bundle(cli)? {
        if approved_via_digest {
            let _ = cli.run(
                "trust revoke",
                &["trust", "revoke", &id, "--kind", "playbook"],
            );
        }
        return Err(InstallError {
            step: "verify approval",
            detail: format!("{id} {version} changed during its approval; the approval was revoked"),
        });
    }
    // 5. Read the installed current version back; it must carry
    //    trial_candidates: off and be the version just installed.
    let back = read_back_export(cli, &id)?;
    if !back.trial_off {
        return Err(InstallError {
            step: "read back",
            detail: format!(
                "the installed version of {id} does not carry trial_candidates: off; refusing before any market call"
            ),
        });
    }
    if back.version != version {
        return Err(InstallError {
            step: "read back",
            detail: format!(
                "the installed current version of {id} reads back as {} but {} was installed",
                back.version, version
            ),
        });
    }
    Ok(Installed {
        playbook_id: id,
        version: back.version,
        sha256: nearbee_core::journal::sha256_hex(materialised.as_bytes()),
    })
}

/// Reads the installed current version back through `apb export`: the
/// bundle JSON's version and playbook text.
pub fn read_back_export(cli: &ApbCli, id: &str) -> Result<ReadBack, InstallError> {
    // A private temp file (random name), removed on drop.
    let out = tempfile::Builder::new()
        .prefix("nearbee-readback-")
        .tempfile()
        .map_err(|e| InstallError {
            step: "read back",
            detail: e.to_string(),
        })?;
    let out_arg = out.path().display().to_string();
    cli.run("export", &["export", id, &out_arg])?;
    let text = std::fs::read_to_string(out.path()).map_err(|e| InstallError {
        step: "read back",
        detail: format!("{}: {e}", out.path().display()),
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| InstallError {
        step: "read back",
        detail: format!("not JSON: {e}"),
    })?;
    let version = v["version"].as_str().unwrap_or_default().to_owned();
    let playbook = v["playbook"].as_str().unwrap_or_default();
    // The trial setting is matched structurally in the parsed YAML.
    let trial_off = trial_candidates_off(playbook) == Some(true);
    Ok(ReadBack { version, trial_off })
}

/// The whole publish-side install: materialise, install, read back.
pub fn install_source(
    cli: &ApbCli,
    source: &str,
    handle: &str,
    scripts: &[Script],
) -> Result<Installed, InstallError> {
    let materialised = materialise(source, handle)?;
    install_playbook(cli, &materialised, scripts)
}

#[cfg(test)]
mod tests {
    use super::*;

    use nearbee_core::journal::sha256_hex;

    fn read_source() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../playbooks/first-hire/repo-audit-fast.yaml");
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn materialise_rewrites_the_id_and_sets_trial_off() {
        let src = read_source();
        let m = materialise(&src, "repo-audit-fast").unwrap();
        assert!(m.contains("id: repo-audit-fast\n"));
        assert!(
            !m.contains("version: 1.0.0"),
            "the version is derived, not copied"
        );
        assert!(m.contains("trial_candidates: off"));
        assert!(
            m.contains("profile: nearbee-own"),
            "profile names are unchanged"
        );
        assert!(m.contains("nodes:\n"), "the rest of the playbook is kept");
        // The version derivation is stable for the same source...
        assert_eq!(materialise(&src, "repo-audit-fast").unwrap(), m);
        let v = version_in(&m).unwrap();
        // ...and a changed source gets a different version.
        let other =
            materialise(&src.replace("nodes:", "nodes_changed:"), "repo-audit-fast").unwrap();
        assert_ne!(version_in(&other).unwrap(), v);
    }

    #[test]
    fn materialise_refuses_a_trial_setting_other_than_off() {
        let src = read_source().replace(
            "defaults:",
            "supervisor:\n  policy:\n    trial_candidates: auto\ndefaults:",
        );
        let err = materialise(&src, "x").unwrap_err();
        assert!(err.detail.contains("trial_candidates"), "{err}");
        // `off` explicitly is fine.
        let src = read_source().replace(
            "defaults:",
            "supervisor:\n  policy:\n    trial_candidates: off\ndefaults:",
        );
        let m = materialise(&src, "x").unwrap();
        assert_eq!(m.matches("trial_candidates: off").count(), 1);
        // The structural check is not fooled by a comment or flow style.
        let tricky = read_source().replace(
            "defaults:",
            "supervisor:\n  policy: {trial_candidates: auto}\ndefaults:",
        );
        assert!(materialise(&tricky, "x").is_err());
        let commented =
            read_source().replace("defaults:", "defaults:\n  # trial_candidates: off\n");
        assert!(
            materialise(&commented, "x").is_ok(),
            "a comment is not a setting"
        );
    }

    #[test]
    fn materialise_rejects_an_unsafe_id() {
        let src = read_source();
        assert!(materialise(&src, "../evil").is_err());
        assert!(materialise(&src, "").is_err());
    }

    /// O-13: a source `policy:` block in the supervisor is dropped whole,
    /// header and children; the adapter's own policy block replaces it.
    #[test]
    fn a_policy_block_is_dropped_with_all_of_its_children() {
        let src = read_source().replace(
            "defaults:",
            "supervisor:\n  policy:\n    trial_candidates: off\n    log_level: debug\ndefaults:",
        );
        let m = materialise(&src, "x").unwrap();
        assert_eq!(m.matches("trial_candidates: off").count(), 1);
        assert!(
            !m.contains("log_level"),
            "a child of the dropped policy block survived: {m}"
        );
        // A deeper nested policy drops only its own subtree.
        let src = read_source().replace(
            "defaults:",
            "supervisor:\n  policy:\n    trial_candidates: off\n      nested: keep_or_drop\ndefaults:",
        );
        let m = materialise(&src, "x").unwrap();
        assert_eq!(m.matches("trial_candidates: off").count(), 1);
        assert!(!m.contains("nested"), "{m}");
    }

    /// L2: a blank or comment-only line inside the source `policy:` block
    /// does not end the skip, so no child after it merges into the
    /// adapter's policy block; the next real key ends it.
    #[test]
    fn a_blank_line_inside_the_policy_block_does_not_end_the_skip() {
        let src = read_source().replace(
            "defaults:",
            "supervisor:\n  policy:\n    max_retries: 2\n\n    # a comment\n    budget: 3\n  keep_me: 1\ndefaults:",
        );
        let m = materialise(&src, "x").unwrap();
        assert!(
            !m.contains("budget"),
            "a child after the blank line survived: {m}"
        );
        assert!(!m.contains("max_retries"), "{m}");
        assert!(
            m.contains("  keep_me: 1"),
            "the sibling after the block was dropped: {m}"
        );
        assert_eq!(m.matches("trial_candidates: off").count(), 1);
        let v: yaml_serde::Value = yaml_serde::from_str(&m).unwrap();
        assert_eq!(
            v["supervisor"]["policy"],
            yaml_serde::from_str::<yaml_serde::Value>("trial_candidates: off").unwrap()
        );
    }

    /// O-13: only `supervisor.policy.trial_candidates` is the trial
    /// setting; the same key anywhere else is not one.
    #[test]
    fn trial_candidates_off_is_accepted_only_in_the_supervisor_policy_block() {
        // At the top level, and directly under supervisor: not the setting.
        for yaml in [
            "trial_candidates: auto\nid: x\n",
            "supervisor:\n  trial_candidates: auto\nid: x\n",
        ] {
            assert_eq!(
                trial_candidates_off(yaml),
                None,
                "a stray key was treated as the setting: {yaml}"
            );
        }
        // The real path is read.
        assert_eq!(
            trial_candidates_off("supervisor:\n  policy:\n    trial_candidates: off\n"),
            Some(true)
        );
        assert_eq!(
            trial_candidates_off("supervisor:\n  policy:\n    trial_candidates: auto\n"),
            Some(false)
        );
        // A playbook with the key only in another block installs fine and
        // carries exactly the adapter's own trial setting.
        let src = read_source().replace("defaults:", "trial_candidates: auto\ndefaults:");
        let m = materialise(&src, "x").unwrap();
        assert_eq!(m.matches("trial_candidates: off").count(), 1);
    }

    /// A fake apb CLI: a shell script that emulates import, trust approve
    /// and export from a state directory (APB_CONFIG_DIR), the S1 fake
    /// pattern. Every call appends its argv to `argv.log`; `trust approve`
    /// accepts only the digest of the installed playbook text.
    struct FakeApb {
        _dir: tempfile::TempDir,
        cli: ApbCli,
    }

    const FAKE: &str = r#"#!/bin/sh
# A fake apb CLI driven by a state directory (APB_CONFIG_DIR).
STATE="${APB_CONFIG_DIR:?}"
printf '%s\n' "$*" >> "$STATE/argv.log"
case "$1" in
  import)
    bundle="$2"
    id="$(sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' "$bundle")"
    ver="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$bundle")"
    if [ "$3" = "--preview" ]; then
      if [ -f "$STATE/holds" ]; then
        echo "bundle $id @ $ver: unchanged: the current version already holds this bundle"
        exit 0
      fi
      echo "bundle $id @ $ver is new"
      exit 1
    fi
    # accept: record the accepted digest for the test to compare.
    if [ "$3" = "--accept" ]; then
      echo "$4" > "$STATE/accepted-digest.txt"
      touch "$STATE/holds"
      echo imported
      exit 0
    fi
    if [ -f "$STATE/exists" ]; then
      echo "version $ver already exists"
      exit 2
    fi
    echo "bundle $id @ $ver is new"
    echo "the imported version carries scripts: it runs only after an approval that names its digest"
    echo "--accept sha256:deadbeef"
    exit 3
    ;;
  trust)
    if [ "$2" = approve ]; then
      if [ "$6" = "--digest" ]; then
        want="$(sha256sum "$STATE/playbook.txt" | cut -d' ' -f1)"
        if [ "$7" = "sha256:$want" ]; then
          touch "$STATE/approved"
          echo approved
          exit 0
        fi
        echo "digest mismatch" >&2
        exit 2
      fi
      want="$(sha256sum "$STATE/playbook.txt" | cut -d' ' -f1)"
      echo "approve with the trust digest:"
      echo "sha256:$want"
      exit 3
    fi
    if [ "$2" = revoke ]; then
      rm -f "$STATE/approved"
      echo revoked
      exit 0
    fi
    echo "fake: trust $2" >&2
    exit 2
    ;;
  export)
    # export <id> <out>
    version="$(cat "$STATE/version.txt" 2>/dev/null || echo 1.0.0)"
    playbook="$(cat "$STATE/playbook.txt" 2>/dev/null || echo "id: x")"
    escaped="$(printf '%s\n' "$playbook" | sed 's/\\/\\\\/g; s/"/\\"/g' | sed ':a;N;$!ba;s/\n/\\n/g')"
    printf '{"apb_bundle":2,"id":"%s","version":"%s","playbook":"%s"}\n' "$2" "$version" "$escaped" > "$3"
    ;;
  *)
    echo "fake apb: unknown command $*" >&2
    exit 2
    ;;
esac
"#;

    fn fake_apb() -> FakeApb {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let bin = dir.path().join("fake-apb.sh");
        std::fs::write(&bin, FAKE).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cli = ApbCli {
            bin,
            workspace: ws,
            // The fake uses the config dir as its state directory.
            config_dir: state,
        };
        FakeApb { _dir: dir, cli }
    }

    impl FakeApb {
        fn state(&self) -> std::path::PathBuf {
            self.cli.config_dir.clone()
        }
    }

    fn scripts() -> Vec<Script> {
        vec![Script {
            name: "manifest.sh".into(),
            bytes: b"echo hi\n".to_vec(),
            executable: true,
        }]
    }

    fn materialised_for(handle: &str) -> String {
        materialise(&read_source(), handle).unwrap()
    }

    /// Puts the materialised text into the fake's export state, so the
    /// read-back of the installed version sees it.
    fn serve_export_state(fake: &FakeApb, materialised: &str) {
        let version = version_in(materialised).unwrap();
        std::fs::write(fake.state().join("version.txt"), &version).unwrap();
        std::fs::write(fake.state().join("playbook.txt"), materialised).unwrap();
    }

    fn expected_trust_digest(materialised: &str) -> String {
        format!("sha256:{}", sha256_hex(materialised.as_bytes()))
    }

    /// A fresh install imports the bundle, approves exactly the installed
    /// playbook (the digest apb names), and reports the installed version.
    #[test]
    fn a_fresh_install_imports_approves_and_reports_the_digest() {
        let fake = fake_apb();
        let m = materialised_for("repo-audit-fast");
        let version = version_in(&m).unwrap();
        serve_export_state(&fake, &m);
        let installed = install_playbook(&fake.cli, &m, &scripts()).unwrap();
        assert_eq!(installed.playbook_id, "repo-audit-fast");
        assert_eq!(installed.version, version);
        assert_eq!(installed.sha256, sha256_hex(m.as_bytes()));
        // The accept named the digest of the bundle just built.
        let accepted = std::fs::read_to_string(fake.state().join("accepted-digest.txt")).unwrap();
        let bundle_digest = format!(
            "sha256:{}",
            sha256_hex(&serde_json::to_vec(&build_bundle(&m, &scripts())).unwrap())
        );
        assert_eq!(accepted.trim(), bundle_digest);
        // The trust approval named exactly the digest of the installed text.
        let argv = std::fs::read_to_string(fake.state().join("argv.log")).unwrap();
        let want = format!(
            "trust approve repo-audit-fast --version {version} --digest {}",
            expected_trust_digest(&m)
        );
        assert!(argv.contains(&want), "argv log: {argv}");
        assert!(fake.state().join("approved").exists());
    }

    /// An answer larger than the pipe buffer never blocks the child: the
    /// pipes are drained while it runs (N10).
    #[test]
    fn a_big_answer_does_not_block_on_the_pipe_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("big-apb.sh");
        // 200001 bytes, well over the roughly 64 KiB pipe buffer.
        std::fs::write(&bin, "#!/bin/sh\nyes x | head -c 200001\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cli = ApbCli {
            bin,
            workspace: dir.path().to_path_buf(),
            config_dir: dir.path().to_path_buf(),
        };
        let out = cli.run("probe", &[]).unwrap();
        assert_eq!(out.len(), 200001, "the whole answer was drained");
    }

    /// An unchanged reinstall: the preview says unchanged, no new import
    /// acceptance is asked for, the version is read back the same.
    #[test]
    fn an_unchanged_reinstall_says_unchanged_and_keeps_the_version() {
        let fake = fake_apb();
        let m = materialised_for("repo-audit-fast");
        let version = version_in(&m).unwrap();
        serve_export_state(&fake, &m);
        std::fs::write(fake.state().join("holds"), "").unwrap();
        let installed = install_playbook(&fake.cli, &m, &scripts()).unwrap();
        assert_eq!(installed.version, version, "the version stays the same");
        assert_eq!(installed.sha256, sha256_hex(m.as_bytes()));
    }

    /// The read back goes through `apb export`, checks the trial setting
    /// structurally and reports the installed version.
    #[test]
    fn the_export_read_back_checks_trial_candidates() {
        let fake = fake_apb();
        let m = materialised_for("repo-audit-fast");
        let version = version_in(&m).unwrap();
        serve_export_state(&fake, &m);
        let back = read_back_export(&fake.cli, "repo-audit-fast").unwrap();
        assert_eq!(back.version, version);
        assert!(back.trial_off);
        // A version without the setting is refused by install.
        std::fs::write(fake.state().join("playbook.txt"), "id: x\nname: y\n").unwrap();
        let back = read_back_export(&fake.cli, "repo-audit-fast").unwrap();
        assert!(!back.trial_off);
    }

    /// An installed version whose read back lacks `trial_candidates: off`
    /// is refused by install before anything else happens.
    #[test]
    fn an_installed_version_without_the_trial_setting_is_refused() {
        let fake = fake_apb();
        let m = materialised_for("repo-audit-fast");
        std::fs::write(fake.state().join("version.txt"), version_in(&m).unwrap()).unwrap();
        std::fs::write(fake.state().join("playbook.txt"), "id: repo-audit-fast\n").unwrap();
        let err = install_playbook(&fake.cli, &m, &scripts()).unwrap_err();
        assert_eq!(err.step, "read back", "{err}");
        assert!(err.detail.contains("trial_candidates"), "{err}");
        // Publishing stops here: no market call follows an install that
        // fails its read back.
    }

    /// A read back of another version than the one installed is refused.
    #[test]
    fn a_read_back_of_another_version_is_refused() {
        let fake = fake_apb();
        let m = materialised_for("repo-audit-fast");
        std::fs::write(fake.state().join("version.txt"), "9.9.9").unwrap();
        std::fs::write(fake.state().join("playbook.txt"), &m).unwrap();
        let err = install_playbook(&fake.cli, &m, &scripts()).unwrap_err();
        assert_eq!(err.step, "read back", "{err}");
        assert!(err.detail.contains("reads back as 9.9.9"), "{err}");
    }

    #[test]
    fn a_version_that_already_exists_with_other_content_is_refused() {
        let fake = fake_apb();
        let m = materialised_for("repo-audit-fast");
        serve_export_state(&fake, &m);
        std::fs::write(fake.state().join("exists"), "").unwrap();
        let err = install_playbook(&fake.cli, &m, &scripts()).unwrap_err();
        assert!(err.detail.contains("already exists"), "{err}");
        // Nothing was approved.
        assert!(!fake.state().join("approved").exists());
    }
}
