//! Configuration: built-in defaults, then the YAML file, then a `.env` file
//! next to it, then the process environment, each later layer overriding the
//! earlier. Relative paths in the file resolve against the file's directory.
//!
//! The `.env` file is read into a map and never written into the process
//! environment, so a variable set in the process always wins.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::secret::Secret;

/// Default config file name, looked up in the working directory.
pub const DEFAULT_CONFIG_FILE: &str = "nearbee.yaml";
/// Default URL of Nearbee's own apb instance (port 7322, never the owner's 7321).
pub const DEFAULT_APB_URL: &str = "http://127.0.0.1:7322";
/// Default dedicated apb workspace directory.
pub const DEFAULT_WORKSPACE_PATH: &str = "~/.local/share/nearbee/apb-workspace";
/// Default state directory.
pub const DEFAULT_STATE_DIR: &str = ".nearbee";

/// A variable Nearbee reads from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarSpec {
    pub name: &'static str,
    pub required: bool,
    pub secret: bool,
    pub purpose: &'static str,
}

/// Every environment variable this stage knows. Later stages add their own.
pub const VARIABLES: &[VarSpec] = &[
    VarSpec {
        name: "APB_URL",
        required: false,
        secret: false,
        purpose: "URL of Nearbee's apb dashboard",
    },
    VarSpec {
        name: "APB_API_KEY",
        required: false,
        secret: true,
        purpose: "apb API key, when the dashboard requires one",
    },
    VarSpec {
        name: "NEARAI_API_KEY",
        required: true,
        secret: true,
        purpose: "NEAR AI Cloud key for the nearbee-near profile",
    },
];

/// Size limits of a result manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    pub markdown_bytes: u64,
    pub file_bytes: u64,
    pub files: usize,
    pub total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            markdown_bytes: 262_144,
            file_bytes: 25 * 1024 * 1024,
            files: 20,
            total_bytes: 100 * 1024 * 1024,
        }
    }
}

/// apb connection settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApbConfig {
    pub url: String,
    /// Absolute path of the apb project Nearbee's playbooks live in.
    pub workspace_path: PathBuf,
    pub api_key: Option<Secret>,
}

/// The loaded configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The YAML file that was read, if any.
    pub file: Option<PathBuf>,
    pub state_dir: PathBuf,
    pub apb: ApbConfig,
    pub limits: Limits,
    pub nearai_api_key: Option<Secret>,
    /// Which known variables are set (by name; values are never kept here).
    pub present: BTreeMap<&'static str, bool>,
}

impl Config {
    /// Every secret value the config holds, for redaction.
    pub fn secrets(&self) -> Vec<Secret> {
        self.apb
            .api_key
            .iter()
            .chain(self.nearai_api_key.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    state_dir: Option<PathBuf>,
    apb: Option<FileApb>,
    limits: Option<Limits>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileApb {
    url: Option<String>,
    workspace_path: Option<PathBuf>,
}

/// A configuration loading failure.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("config file {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error(".env file {path}: {message}")]
    DotEnv { path: PathBuf, message: String },
    #[error("cannot expand `~` in {path}: HOME is not set")]
    NoHome { path: PathBuf },
    /// The apb URL carries a user name or password. The value is never
    /// shown; `origin` names where it came from.
    #[error("{origin} must not carry credentials (`user:password@`); give the key in APB_API_KEY")]
    UrlCredentials { origin: String },
    /// The apb URL is not an absolute `http` or `https` URL. The value is
    /// never shown.
    #[error("{origin} must be an absolute http or https URL")]
    UrlInvalid { origin: String },
    /// The file holds a token-like value (`aat_...`). Configuration names
    /// agents by handle only; the value is never shown.
    #[error(
        "config file {path} holds an agent token; tokens belong in the credentials store (`nearbee agent add`), not the config"
    )]
    TokenInFile { path: PathBuf },
}

/// Where to read the configuration from.
#[derive(Debug, Clone)]
pub struct LoadOptions<'a> {
    /// `--config`; when `None`, `nearbee.yaml` in `cwd` is read if it exists.
    pub file: Option<&'a Path>,
    pub cwd: &'a Path,
}

/// Loads the configuration. `env` is the process environment lookup
/// (`std::env::var(..).ok()` in production, a map in tests).
pub fn load(
    opts: LoadOptions<'_>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Config, ConfigError> {
    let (file_path, explicit) = match opts.file {
        Some(p) => (absolute(p, opts.cwd), true),
        None => (opts.cwd.join(DEFAULT_CONFIG_FILE), false),
    };

    let file_cfg = if explicit || file_path.exists() {
        let text = std::fs::read_to_string(&file_path).map_err(|source| ConfigError::Read {
            path: file_path.clone(),
            source,
        })?;
        if crate::credentials::find_token_like(&text).is_some() {
            return Err(ConfigError::TokenInFile {
                path: file_path.clone(),
            });
        }
        let parsed: FileConfig = if text.trim().is_empty() {
            FileConfig::default()
        } else {
            yaml_serde::from_str(&text).map_err(|e| ConfigError::Parse {
                path: file_path.clone(),
                message: e.to_string(),
            })?
        };
        Some(parsed)
    } else {
        None
    };
    let base_dir = if file_cfg.is_some() {
        file_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| opts.cwd.to_path_buf())
    } else {
        opts.cwd.to_path_buf()
    };

    let dotenv = read_dotenv(&base_dir.join(".env"))?;
    let var = |name: &str| -> Option<String> {
        // An empty value counts as unset in each layer, so a set-but-empty
        // process variable does not hide the `.env` value.
        env(name)
            .filter(|v| !v.is_empty())
            .or_else(|| dotenv.get(name).filter(|v| !v.is_empty()).cloned())
    };

    let file_cfg = file_cfg.unwrap_or_default();
    let file_apb = file_cfg.apb.unwrap_or_default();
    let home = env("HOME");

    let state_dir = resolve_path(
        file_cfg
            .state_dir
            .as_deref()
            .unwrap_or(Path::new(DEFAULT_STATE_DIR)),
        &base_dir,
        home.as_deref(),
    )?;
    let workspace_path = resolve_path(
        file_apb
            .workspace_path
            .as_deref()
            .unwrap_or(Path::new(DEFAULT_WORKSPACE_PATH)),
        &base_dir,
        home.as_deref(),
    )?;
    let url = match (var("APB_URL"), file_apb.url) {
        (Some(u), _) => checked_url(u, || "APB_URL".to_owned())?,
        (None, Some(u)) => checked_url(u, || format!("apb.url in {}", file_path.display()))?,
        (None, None) => DEFAULT_APB_URL.to_owned(),
    };

    let present = VARIABLES
        .iter()
        .map(|v| (v.name, var(v.name).is_some()))
        .collect();

    Ok(Config {
        file: (explicit || file_path.exists()).then_some(file_path),
        state_dir,
        apb: ApbConfig {
            url,
            workspace_path,
            api_key: var("APB_API_KEY").map(Secret::new),
        },
        limits: file_cfg.limits.unwrap_or_default(),
        nearai_api_key: var("NEARAI_API_KEY").map(Secret::new),
        present,
    })
}

/// Accepts only an absolute `http` or `https` URL without userinfo
/// (`user:password@host`): a credential there would be printed wherever the
/// URL is. The check parses with the `url` crate, the parser reqwest uses, so
/// both agree on where the authority is (extra slashes, backslashes, case).
/// Errors never carry the value.
fn checked_url(url: String, origin: impl FnOnce() -> String) -> Result<String, ConfigError> {
    let Ok(parsed) = url::Url::parse(&url) else {
        return Err(ConfigError::UrlInvalid { origin: origin() });
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ConfigError::UrlInvalid { origin: origin() });
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ConfigError::UrlCredentials { origin: origin() });
    }
    Ok(url)
}

fn read_dotenv(path: &Path) -> Result<BTreeMap<String, String>, ConfigError> {
    let mut map = BTreeMap::new();
    if !path.exists() {
        return Ok(map);
    }
    let iter = dotenvy::from_path_iter(path).map_err(|e| ConfigError::DotEnv {
        path: path.to_path_buf(),
        message: dotenv_message(&e),
    })?;
    for item in iter {
        let (k, v) = item.map_err(|e| ConfigError::DotEnv {
            path: path.to_path_buf(),
            message: dotenv_message(&e),
        })?;
        map.insert(k, v);
    }
    Ok(map)
}

/// A dotenvy parse error quotes the offending line, which may hold a secret;
/// keep only the position.
fn dotenv_message(e: &dotenvy::Error) -> String {
    match e {
        dotenvy::Error::LineParse(_, index) => format!("cannot parse the line at byte {index}"),
        dotenvy::Error::Io(io) => io.to_string(),
        _ => "cannot read the file".to_owned(),
    }
}

fn absolute(p: &Path, cwd: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

fn resolve_path(p: &Path, base: &Path, home: Option<&str>) -> Result<PathBuf, ConfigError> {
    let mut comps = p.components();
    let expanded = match comps.next() {
        Some(Component::Normal(first)) if first == "~" => {
            let home = home.ok_or_else(|| ConfigError::NoHome {
                path: p.to_path_buf(),
            })?;
            Path::new(home).join(comps.as_path())
        }
        _ => absolute(p, base),
    };
    Ok(normalize(&expanded))
}

/// Removes `.` components and folds `..` lexically (no filesystem access).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let mut map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        map.entry("HOME".into()).or_insert_with(|| "/home/u".into());
        move |k| map.get(k).cloned()
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn credentials_in_the_apb_url_are_refused_by_name_without_the_value() {
        let dir = tempfile::tempdir().unwrap();
        let opts = || LoadOptions {
            file: None,
            cwd: dir.path(),
        };
        for url in [
            "http://operator:synthetic-secret@127.0.0.1:7322",
            "https://synthetic-secret@127.0.0.1:7322/",
            "synthetic-secret@127.0.0.1:7322",
        ] {
            let err = load(opts(), env_of(&[("APB_URL", url)])).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("APB_URL"), "{msg}");
            assert!(!msg.contains("synthetic-secret"), "{msg}");
        }
        write(
            dir.path(),
            "nearbee.yaml",
            "apb:\n  url: http://u:synthetic-secret@127.0.0.1:7322\n",
        );
        let msg = load(opts(), env_of(&[])).unwrap_err().to_string();
        assert!(msg.contains("apb.url"), "{msg}");
        assert!(!msg.contains("synthetic-secret"), "{msg}");
        // An `@` after the authority is no credential.
        std::fs::remove_file(dir.path().join("nearbee.yaml")).unwrap();
        let url = "http://127.0.0.1:7322/x@y?q=a@b#c@d";
        let cfg = load(opts(), env_of(&[("APB_URL", url)])).unwrap();
        assert_eq!(cfg.apb.url, url);
    }

    #[test]
    fn credentials_behind_extra_slashes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        for url in [
            "http:////operator:synthetic-secret@127.0.0.1:1",
            "http:/operator:synthetic-secret@127.0.0.1:1",
            "https:\\\\operator:synthetic-secret@127.0.0.1:1",
            "HTTP://operator:synthetic-secret@127.0.0.1:1",
        ] {
            let err = load(
                LoadOptions {
                    file: None,
                    cwd: dir.path(),
                },
                env_of(&[("APB_URL", url)]),
            )
            .unwrap_err();
            assert!(
                matches!(err, ConfigError::UrlCredentials { .. }),
                "{url}: {err}"
            );
            assert!(!err.to_string().contains("synthetic-secret"), "{err}");
        }
    }

    #[test]
    fn the_apb_url_must_be_an_absolute_http_url() {
        let dir = tempfile::tempdir().unwrap();
        for url in [
            // Scheme-less, with `://` and `@` only in the query: not a
            // credential, but not a usable URL either.
            "127.0.0.1:7322/?next=http://a@b",
            "synthetic-value",
            "ftp://127.0.0.1:7322",
            "unix:/run/apb.sock",
        ] {
            let err = load(
                LoadOptions {
                    file: None,
                    cwd: dir.path(),
                },
                env_of(&[("APB_URL", url)]),
            )
            .unwrap_err();
            assert!(
                matches!(err, ConfigError::UrlInvalid { .. }),
                "{url}: {err}"
            );
            let msg = err.to_string();
            assert!(msg.contains("APB_URL"), "{msg}");
            assert!(!msg.contains(url), "{msg}");
        }
        for url in [
            "http://127.0.0.1:7322",
            "https://apb.example:8443/base/",
            "http://localhost:7322/?next=http://a@b",
        ] {
            let cfg = load(
                LoadOptions {
                    file: None,
                    cwd: dir.path(),
                },
                env_of(&[("APB_URL", url)]),
            )
            .unwrap();
            assert_eq!(cfg.apb.url, url);
        }
    }

    #[test]
    fn defaults_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(
            LoadOptions {
                file: None,
                cwd: dir.path(),
            },
            env_of(&[("HOME", "/home/u")]),
        )
        .unwrap();
        assert_eq!(cfg.file, None);
        assert_eq!(cfg.apb.url, DEFAULT_APB_URL);
        assert_eq!(
            cfg.apb.workspace_path,
            PathBuf::from("/home/u/.local/share/nearbee/apb-workspace")
        );
        assert_eq!(cfg.state_dir, dir.path().join(".nearbee"));
        assert_eq!(cfg.limits, Limits::default());
        assert_eq!(cfg.limits.markdown_bytes, 262_144);
        assert!(!cfg.present["NEARAI_API_KEY"]);
    }

    #[test]
    fn process_environment_wins_over_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(
            dir.path(),
            "nearbee.yaml",
            "apb:\n  url: http://127.0.0.1:9\n",
        );
        write(dir.path(), ".env", "APB_URL=http://127.0.0.1:1\n");
        let cfg = load(
            LoadOptions {
                file: Some(&file),
                cwd: Path::new("/"),
            },
            env_of(&[("APB_URL", "http://127.0.0.1:7321")]),
        )
        .unwrap();
        assert_eq!(cfg.apb.url, "http://127.0.0.1:7321");
    }

    #[test]
    fn empty_process_variable_does_not_hide_the_dotenv_value() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "nearbee.yaml", "");
        write(
            dir.path(),
            ".env",
            "APB_URL=http://127.0.0.1:1\nAPB_API_KEY=apb_fromdotenv\n",
        );
        let cfg = load(
            LoadOptions {
                file: Some(&file),
                cwd: Path::new("/"),
            },
            env_of(&[("APB_URL", ""), ("APB_API_KEY", "")]),
        )
        .unwrap();
        assert_eq!(cfg.apb.url, "http://127.0.0.1:1");
        assert_eq!(
            cfg.apb.api_key.as_ref().map(|k| k.expose().to_owned()),
            Some("apb_fromdotenv".to_owned())
        );
        assert!(cfg.present["APB_API_KEY"]);
    }

    #[test]
    fn dotenv_wins_over_the_file_and_the_file_over_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(
            dir.path(),
            "nearbee.yaml",
            "apb:\n  url: http://127.0.0.1:9\n",
        );
        let cfg = load(
            LoadOptions {
                file: Some(&file),
                cwd: Path::new("/"),
            },
            env_of(&[]),
        )
        .unwrap();
        assert_eq!(cfg.apb.url, "http://127.0.0.1:9");
        write(dir.path(), ".env", "APB_URL=http://127.0.0.1:1\n");
        let cfg = load(
            LoadOptions {
                file: Some(&file),
                cwd: Path::new("/"),
            },
            env_of(&[]),
        )
        .unwrap();
        assert_eq!(cfg.apb.url, "http://127.0.0.1:1");
    }

    #[test]
    fn relative_paths_resolve_against_the_file_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("srv/nb");
        std::fs::create_dir_all(&sub).unwrap();
        let file = write(
            &sub,
            "nearbee.yaml",
            "state_dir: .nearbee\napb:\n  workspace_path: ../ws\n",
        );
        let other = tempfile::tempdir().unwrap();
        let cfg = load(
            LoadOptions {
                file: Some(&file),
                cwd: other.path(),
            },
            env_of(&[]),
        )
        .unwrap();
        assert_eq!(cfg.state_dir, sub.join(".nearbee"));
        assert_eq!(cfg.apb.workspace_path, dir.path().join("srv/ws"));
    }

    #[test]
    fn unknown_key_names_the_key_and_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "nearbee.yaml", "apb:\n  urll: x\n");
        let err = load(
            LoadOptions {
                file: Some(&file),
                cwd: dir.path(),
            },
            env_of(&[]),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("urll"), "{msg}");
        assert!(msg.contains(&file.display().to_string()), "{msg}");

        let file = write(dir.path(), "top.yaml", "colour: red\n");
        let msg = load(
            LoadOptions {
                file: Some(&file),
                cwd: dir.path(),
            },
            env_of(&[]),
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("colour") && msg.contains("top.yaml"), "{msg}");
    }

    #[test]
    fn explicit_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.yaml");
        let err = load(
            LoadOptions {
                file: Some(&missing),
                cwd: dir.path(),
            },
            env_of(&[]),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
    }

    #[test]
    fn debug_output_of_a_loaded_config_hides_secrets() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".env", "NEARAI_API_KEY=sk-abc\n");
        let cfg = load(
            LoadOptions {
                file: None,
                cwd: dir.path(),
            },
            env_of(&[("APB_API_KEY", "apb_key_xyz")]),
        )
        .unwrap();
        assert_eq!(cfg.nearai_api_key.as_ref().unwrap().expose(), "sk-abc");
        assert!(cfg.present["NEARAI_API_KEY"]);
        let dbg = format!("{cfg:?} {cfg:#?}");
        assert!(!dbg.contains("sk-abc"), "{dbg}");
        assert!(!dbg.contains("apb_key_xyz"), "{dbg}");
    }

    #[test]
    fn dotenv_parse_error_does_not_quote_the_line() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".env", "GOOD=1\nBAD LINE sk-leak'\n");
        let err = load(
            LoadOptions {
                file: None,
                cwd: dir.path(),
            },
            env_of(&[]),
        )
        .unwrap_err();
        assert!(!err.to_string().contains("sk-leak"), "{err}");
    }

    #[test]
    fn limits_are_configurable_and_partial() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "nearbee.yaml", "limits:\n  files: 3\n");
        let cfg = load(
            LoadOptions {
                file: Some(&file),
                cwd: dir.path(),
            },
            env_of(&[]),
        )
        .unwrap();
        assert_eq!(cfg.limits.files, 3);
        assert_eq!(cfg.limits.markdown_bytes, 262_144);
    }

    #[test]
    fn a_token_like_value_in_the_config_is_refused_without_the_value() {
        let dir = tempfile::tempdir().unwrap();
        let tok = "aat_0123456789abcdef0123456789abcdef";
        let file = write(
            dir.path(),
            "nearbee.yaml",
            &format!("apb:\n  url: http://127.0.0.1:7322 # {tok}\n"),
        );
        let err = load(
            LoadOptions {
                file: Some(&file),
                cwd: dir.path(),
            },
            env_of(&[]),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("token"), "{msg}");
        assert!(msg.contains(&file.display().to_string()), "{msg}");
        assert!(!msg.contains(tok), "{msg}");
        // An uppercase or short value is not token-shaped and passes.
        let file = write(
            dir.path(),
            "nearbee.yaml",
            "apb:\n  url: http://127.0.0.1:7322 # AAT_0123\n",
        );
        assert!(
            load(
                LoadOptions {
                    file: Some(&file),
                    cwd: dir.path(),
                },
                env_of(&[]),
            )
            .is_ok()
        );
    }

    #[test]
    fn example_file_loads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let example = root.join("nearbee.example.yaml");
        let cfg = load(
            LoadOptions {
                file: Some(&example),
                cwd: Path::new("/"),
            },
            env_of(&[("HOME", "/home/u")]),
        )
        .unwrap();
        assert_eq!(cfg.apb.url, DEFAULT_APB_URL);
        assert_eq!(
            cfg.apb.workspace_path,
            PathBuf::from("/home/u/.local/share/nearbee/apb-workspace")
        );
        assert_eq!(cfg.limits, Limits::default());
        let env_example = std::fs::read_to_string(root.join(".env.example")).unwrap();
        for v in VARIABLES {
            assert!(
                env_example.contains(v.name),
                "{} not in .env.example",
                v.name
            );
        }
    }
}
