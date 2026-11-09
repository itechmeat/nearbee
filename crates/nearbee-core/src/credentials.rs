//! The credentials store: one JSON file mapping handles to `{agent_id,
//! token}`, written atomically with mode 0600. Reading refuses a file whose
//! mode grants any access to group or others, so a token never lives in a
//! file others can read.
//!
//! No token value is ever formatted into an error; errors name the file and
//! the expectation.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::secret::Secret;

/// The expected file mode.
pub const EXPECTED_MODE: u32 = 0o600;

/// The path `$XDG_CONFIG_HOME/nearbee/credentials` (default
/// `$HOME/.config/nearbee/credentials`).
pub fn credentials_path(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match xdg_config_home.filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(home.filter(|v| !v.is_empty())?).join(".config"),
    };
    Some(base.join("nearbee").join("credentials"))
}

/// One stored agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCredentials {
    pub agent_id: String,
    pub token: Secret,
}

/// A credentials store failure. Errors never carry a token value.
#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error(
        "credentials file {path} has mode {found:o}; expected {EXPECTED_MODE:o}: no access for group or others"
    )]
    WiderMode { path: PathBuf, found: u32 },
    #[error("credentials file {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("credentials file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("credentials file {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("token for {handle} is not an agent token")]
    BadToken { handle: String },
}

impl CredentialsError {
    /// Line and column only: a parse error's message can echo a token
    /// stored under a wrong shape, and it is never printed.
    fn parse(path: &Path, source: serde_json::Error) -> Self {
        Self::Parse {
            path: path.to_path_buf(),
            message: format!(
                "invalid JSON at line {}, column {} ({})",
                source.line(),
                source.column(),
                match source.classify() {
                    serde_json::error::Category::Io => "io",
                    serde_json::error::Category::Syntax => "syntax",
                    serde_json::error::Category::Data => "data",
                    serde_json::error::Category::Eof => "eof",
                }
            ),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WireAgent {
    agent_id: String,
    token: String,
}

/// Finds a token-like value (`aat_` followed by 32 lowercase hex digits)
/// in configuration or listing text. Returns it, for the caller to refuse
/// it by name without printing the value.
pub fn find_token_like(text: &str) -> Option<String> {
    const PREFIX: &str = "aat_";
    let mut rest = text;
    while let Some(pos) = rest.find(PREFIX) {
        let tail = &rest[pos + PREFIX.len()..];
        let hex: String = tail
            .chars()
            .take(32)
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if hex.len() == 32
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Some(format!("{PREFIX}{hex}"));
        }
        rest = tail;
    }
    None
}

/// The token shape the market mints: `aat_` followed by 32 hex digits.
pub fn token_shape_ok(token: &str) -> bool {
    let hex = token.strip_prefix("aat_").unwrap_or_default();
    hex.len() == 32
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A store bound to one file path.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl fmt::Display for AgentCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.agent_id, masked(&self.token))
    }
}

/// The token shown only as `aat_...` plus its last four characters.
pub fn masked(token: &Secret) -> String {
    let v = token.expose();
    let tail: String = v
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let head = v.split('_').next().unwrap_or("");
    if head.len() < v.len() {
        format!("{head}_...{tail}")
    } else {
        format!("...{tail}")
    }
}

impl Store {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the whole store. A file wider than [`EXPECTED_MODE`] is
    /// refused before its contents are read.
    pub fn read(&self) -> Result<BTreeMap<String, AgentCredentials>, CredentialsError> {
        let meta = fs::metadata(&self.path).map_err(|source| CredentialsError::Read {
            path: self.path.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(CredentialsError::WiderMode {
                    path: self.path.clone(),
                    found: mode,
                });
            }
        }
        let text = fs::read_to_string(&self.path).map_err(|source| CredentialsError::Read {
            path: self.path.clone(),
            source,
        })?;
        let wire: BTreeMap<String, WireAgent> =
            serde_json::from_str(&text).map_err(|e| CredentialsError::parse(&self.path, e))?;
        let mut out = BTreeMap::new();
        for (handle, a) in wire {
            if !token_shape_ok(&a.token) {
                return Err(CredentialsError::BadToken { handle });
            }
            out.insert(
                handle,
                AgentCredentials {
                    agent_id: a.agent_id,
                    token: Secret::new(a.token),
                },
            );
        }
        Ok(out)
    }

    /// Reads one handle's credentials; `None` when the handle is absent.
    pub fn get(&self, handle: &str) -> Result<Option<AgentCredentials>, CredentialsError> {
        Ok(self.read()?.remove(handle))
    }

    /// Writes `handle` atomically: a temp file in the same directory with
    /// mode 0600, fsync, rename. Every other handle is kept byte for byte.
    /// A store that cannot be read (wider mode, corrupt JSON) is never
    /// replaced: the error propagates, so tokens are not lost.
    pub fn put(&self, handle: &str, creds: AgentCredentials) -> Result<(), CredentialsError> {
        if !token_shape_ok(creds.token.expose()) {
            return Err(CredentialsError::BadToken {
                handle: handle.to_owned(),
            });
        }
        // One writer at a time within the process, across the whole
        // read-modify-write: the lock is taken before the read, so two
        // concurrent puts cannot drop each other's handle.
        let _lock = store_lock();
        let mut all = match self.read() {
            Ok(all) => all,
            Err(CredentialsError::Read { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                BTreeMap::new()
            }
            Err(e) => return Err(e),
        };
        all.insert(handle.to_owned(), creds);
        self.write_all(&all)
    }

    /// Writes the whole map atomically.
    fn write_all(&self, all: &BTreeMap<String, AgentCredentials>) -> Result<(), CredentialsError> {
        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(dir).map_err(|source| CredentialsError::Write {
            path: self.path.clone(),
            source,
        })?;
        // The directory holds tokens: private to the owner (0700), never
        // whatever the umask leaves.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        let wire: BTreeMap<&String, WireAgent> = all
            .iter()
            .map(|(h, a)| {
                (
                    h,
                    WireAgent {
                        agent_id: a.agent_id.clone(),
                        token: a.token.expose().to_owned(),
                    },
                )
            })
            .collect();
        let text = serde_json::to_string_pretty(&wire).expect("credentials serialize");
        let (tmp_path, mut tmp_file) =
            tempfile_in(dir).map_err(|source| CredentialsError::Write {
                path: self.path.clone(),
                source,
            })?;
        let written = write_synced(&mut tmp_file, text.as_bytes()).and_then(|_| {
            set_mode_600(&tmp_path);
            persist(&tmp_path, &self.path)
        });
        if let Err(source) = written {
            // The token-bearing temp file never stays behind.
            let _ = fs::remove_file(&tmp_path);
            return Err(CredentialsError::Write {
                path: self.path.clone(),
                source,
            });
        }
        Ok(())
    }
}

/// A unique temp file in `dir` with a random name, created with mode 0600
/// and removed on drop, so a failed write never leaves a token file behind.
fn tempfile_in(dir: &Path) -> Result<(tempfile::TempPath, fs::File), std::io::Error> {
    #[cfg(unix)]
    {
        let f = tempfile::NamedTempFile::with_prefix_in(".credentials.", dir)?;
        use std::os::unix::fs::PermissionsExt;
        f.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        let (f, path) = f.into_parts();
        Ok((path, f))
    }
    #[cfg(not(unix))]
    {
        let f = tempfile::NamedTempFile::with_prefix_in(".credentials.", dir)?;
        let (f, path) = f.into_parts();
        Ok((path, f))
    }
}

/// The process-wide single-writer guard for the credentials store (one
/// read-modify-write at a time within the process).
fn store_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn write_synced(f: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    f.write_all(bytes)?;
    f.sync_all()
}

fn set_mode_600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

/// Renames the temp file over the destination and fsyncs its directory.
fn persist(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    fs::rename(tmp, dest)?;
    if let Some(dir) = dest.parent()
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path) -> Store {
        let p = dir.join("nearbee").join("credentials");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        Store::at(p)
    }

    #[test]
    fn token_format_is_enforced() {
        assert!(token_shape_ok("aat_0123456789abcdef0123456789abcdef"));
        assert!(
            !token_shape_ok("aat_0123456789ABCDEF0123456789abcdef"),
            "not hex only"
        );
        assert!(
            !token_shape_ok("aat_0123456789abcdef0123456789abcde"),
            "too short"
        );
        assert!(!token_shape_ok("sk-0123456789abcdef0123456789abcdef"));
        assert!(!token_shape_ok("aat_"));
    }

    #[test]
    fn masked_shows_only_the_last_four() {
        let t = Secret::new("aat_0123456789abcdef0123456789abcdef");
        assert_eq!(masked(&t), "aat_...cdef");
        assert!(!masked(&t).contains("0123456789"));
    }

    #[test]
    fn put_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.put(
            "repo-audit-fast",
            AgentCredentials {
                agent_id: "ag_1".into(),
                token: Secret::new("aat_0123456789abcdef0123456789abcdef"),
            },
        )
        .unwrap();
        let got = s.get("repo-audit-fast").unwrap().unwrap();
        assert_eq!(got.agent_id, "ag_1");
        assert_eq!(got.token.expose(), "aat_0123456789abcdef0123456789abcdef");
        // The file mode is exactly 0600.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(s.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, EXPECTED_MODE);
        }
    }

    #[test]
    fn a_wider_mode_is_refused_without_reading_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        std::fs::write(
            s.path(),
            r#"{"h":{"agent_id":"a","token":"aat_0123456789abcdef0123456789abcdef"}}"#,
        )
        .unwrap();
        for mode in [0o644, 0o666, 0o604, 0o640] {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(s.path(), fs::Permissions::from_mode(mode)).unwrap();
            }
            let err = s.read().unwrap_err();
            let msg = err.to_string();
            assert!(
                matches!(err, CredentialsError::WiderMode { .. }),
                "{mode}: {msg}"
            );
            assert!(msg.contains("600"), "{msg}");
            assert!(
                msg.contains(&format!("{mode:o}")),
                "{msg} does not name the found mode {mode:o}"
            );
            assert!(!msg.contains("aat_"), "{msg}");
        }
    }

    #[test]
    fn replacing_one_handle_keeps_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        for (h, id, tok) in [
            ("a", "ag_a", "aat_00000000000000000000000000000001"),
            ("b", "ag_b", "aat_00000000000000000000000000000002"),
        ] {
            s.put(
                h,
                AgentCredentials {
                    agent_id: id.into(),
                    token: Secret::new(tok),
                },
            )
            .unwrap();
        }
        s.put(
            "a",
            AgentCredentials {
                agent_id: "ag_a2".into(),
                token: Secret::new("aat_00000000000000000000000000000003"),
            },
        )
        .unwrap();
        let all = s.read().unwrap();
        assert_eq!(all["a"].agent_id, "ag_a2");
        assert_eq!(
            all["a"].token.expose(),
            "aat_00000000000000000000000000000003"
        );
        assert_eq!(all["b"].agent_id, "ag_b");
        assert_eq!(
            all["b"].token.expose(),
            "aat_00000000000000000000000000000002"
        );
        // No temp file is left behind.
        let dir_entries: Vec<_> = s.path().parent().unwrap().read_dir().unwrap().collect();
        assert_eq!(dir_entries.len(), 1, "a temp file was left behind");
    }

    #[test]
    fn a_bad_token_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        let err = s
            .put(
                "h",
                AgentCredentials {
                    agent_id: "a".into(),
                    token: Secret::new("not-a-token"),
                },
            )
            .unwrap_err();
        assert!(matches!(err, CredentialsError::BadToken { .. }), "{err}");
        assert!(!s.path().exists());
    }

    /// Two concurrent puts keep each other's handle: the lock covers the
    /// read as well as the write (N8).
    #[test]
    fn concurrent_puts_keep_every_handle() {
        let dir = tempfile::tempdir().unwrap();
        let s = std::sync::Arc::new(store(dir.path()));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let s = s.clone();
                std::thread::spawn(move || {
                    s.put(
                        &format!("h{i}"),
                        AgentCredentials {
                            agent_id: format!("ag_{i}"),
                            token: Secret::new(format!("aat_{i:032}")),
                        },
                    )
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap().unwrap();
        }
        let all = s.read().unwrap();
        assert_eq!(all.len(), 8, "{:?}", all.keys().collect::<Vec<_>>());
        for i in 0..8 {
            assert_eq!(all[&format!("h{i}")].agent_id, format!("ag_{i}"));
        }
    }

    /// The credentials directory is 0700: the umask never makes it
    /// world-listable (ocr2 7).
    #[cfg(unix)]
    #[test]
    fn the_credentials_directory_is_0700() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.put(
            "h",
            AgentCredentials {
                agent_id: "a".into(),
                token: Secret::new("aat_0123456789abcdef0123456789abcdef"),
            },
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(s.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{mode:o}");
    }

    #[test]
    fn credentials_path_resolves_xdg_and_home() {
        assert_eq!(
            credentials_path(Some("/cfg"), Some("/home/u")).unwrap(),
            PathBuf::from("/cfg/nearbee/credentials")
        );
        assert_eq!(
            credentials_path(None, Some("/home/u")).unwrap(),
            PathBuf::from("/home/u/.config/nearbee/credentials")
        );
        assert_eq!(credentials_path(None, None), None);
        assert_eq!(credentials_path(Some(""), None), None);
    }
}
