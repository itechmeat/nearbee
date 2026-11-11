//! `nearbee config check` and `nearbee runs find` against wiremock serving
//! answers recorded from apb (`crates/nearbee-apb/tests/fixtures/apb/`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use nearbee::{Cli, Env};
use nearbee_apb::Timing;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_WS_PATH: &str = "<home>/.local/share/nearbee/apb-workspace";
const WS: &str = "ws-c0188762df734bf4a1f03f8db22f19d7";

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nearbee-apb/tests/fixtures/apb")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// A config directory with `nearbee.yaml` pointing at `apb_url` and a real
/// workspace directory, and the project list naming that directory.
struct Setup {
    dir: tempfile::TempDir,
    workspace: PathBuf,
}

impl Setup {
    fn new(apb_url: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("apb-workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        std::fs::write(
            dir.path().join("nearbee.yaml"),
            format!(
                "apb:\n  url: {apb_url}\n  workspace_path: {}\n",
                workspace.display()
            ),
        )
        .unwrap();
        Self { dir, workspace }
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("nearbee.yaml")
    }

    /// The recorded project list with the workspace path made real.
    fn projects(&self) -> Value {
        let text = fixture("projects")["response"]["body"]
            .to_string()
            .replace(FIXTURE_WS_PATH, &self.workspace.to_string_lossy());
        serde_json::from_str(&text).unwrap()
    }
}

async fn mount_projects(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// The recorded beta.7 health answer with `version` replaced.
async fn mount_health(server: &MockServer, version: &str) {
    let mut body = fixture("health_beta7_live")["response"]["body"].clone();
    body["version"] = json!(version);
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn run(args: &[&str], cwd: &Path, vars: &[(&str, &str)]) -> (i32, String) {
    let map: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .chain([("HOME".to_string(), "/home/u".to_string())])
        .collect();
    let var = move |k: &str| map.get(k).cloned();
    let env = Env {
        cwd,
        var: &var,
        timing: Timing {
            connect: Duration::from_millis(500),
            read: Duration::from_secs(2),
            estimate: Duration::from_secs(2),
            start: Duration::from_secs(2),
            abort: Duration::from_secs(5),
            decide: Duration::from_secs(5),
            read_retry_delays: vec![Duration::ZERO; 3],
        },
    };
    let mut argv = vec!["nearbee"];
    argv.extend_from_slice(args);
    let cli = Cli::try_parse_from(argv).unwrap();
    let mut out = Vec::new();
    let code = nearbee::run(cli, &env, &mut out).await;
    (code, String::from_utf8(out).unwrap())
}

/// An address nothing listens on: port 1 is privileged, so no test server
/// (all bind unprivileged ports) can take it.
const DEAD_URL: &str = "http://127.0.0.1:1";

#[tokio::test]
async fn config_check_all_present() {
    let server = MockServer::start().await;
    let s = Setup::new(&server.uri());
    mount_projects(&server, s.projects()).await;
    mount_health(&server, "1.0.0-beta.7").await;
    let cfg = s.config();
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "config", "check"],
        Path::new("/"),
        &[
            ("NEARAI_API_KEY", "sk-secret-value"),
            ("APB_API_KEY", "apb_secret"),
            ("APB_URL", &server.uri()),
        ],
    )
    .await;
    assert_eq!(code, 0, "{out}");
    for l in out.lines() {
        assert!(l.starts_with("ok "), "{l}");
    }
    assert!(out.contains("NEARAI_API_KEY is set"), "{out}");
    assert!(out.contains(WS), "{out}");
    assert!(
        out.contains("ok      apb version 1.0.0-beta.7 (external data runs supported)"),
        "{out}"
    );
    assert!(!out.contains("sk-secret-value") && !out.contains("apb_secret"));
}

#[tokio::test]
async fn config_check_reports_missing_variables_by_name() {
    let server = MockServer::start().await;
    let s = Setup::new(&server.uri());
    mount_projects(&server, s.projects()).await;
    mount_health(&server, "1.0.0-beta.7").await;
    let cfg = s.config();
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "config", "check"],
        Path::new("/"),
        &[],
    )
    .await;
    assert_ne!(code, 0, "a required variable is missing: {out}");
    assert!(out.contains("missing NEARAI_API_KEY (required)"), "{out}");
    assert!(out.contains("missing APB_API_KEY (optional)"), "{out}");
}

#[tokio::test]
async fn config_check_dashboard_down() {
    let url = DEAD_URL;
    let s = Setup::new(url);
    let cfg = s.config();
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "config", "check"],
        Path::new("/"),
        &[("NEARAI_API_KEY", "sk-synthetic-test-key")],
    )
    .await;
    assert_ne!(code, 0, "{out}");
    let line = out
        .lines()
        .find(|l| l.contains("apb dashboard http"))
        .unwrap();
    assert!(line.starts_with("failed"), "{line}");
    assert!(line.contains(url), "{line}");
}

#[tokio::test]
async fn config_check_missing_workspace() {
    let server = MockServer::start().await;
    let s = Setup::new(&server.uri());
    // apb knows only the recorded projects, not this workspace.
    mount_projects(&server, fixture("projects")["response"]["body"].clone()).await;
    mount_health(&server, "1.0.0-beta.7").await;
    let cfg = s.config();
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "config", "check"],
        Path::new("/"),
        &[("NEARAI_API_KEY", "sk-synthetic-test-key")],
    )
    .await;
    assert_ne!(code, 0, "{out}");
    let line = out.lines().find(|l| l.contains("apb workspace")).unwrap();
    assert!(line.starts_with("failed"), "{line}");
    assert!(
        line.contains(&s.workspace.to_string_lossy().into_owned()),
        "{line}"
    );
    assert!(line.contains(FIXTURE_WS_PATH), "{line}");
    assert!(line.contains("scratch-project"), "{line}");
}

/// An apb older than beta.7 fails the check, naming the minimum; an
/// unparsable version fails it too.
#[tokio::test]
async fn config_check_fails_an_apb_without_external_data_runs() {
    for (version, want) in [
        (
            "1.0.0-beta.6",
            "failed  apb version 1.0.0-beta.6: Nearbee needs 1.0.0-beta.7 or newer",
        ),
        ("dev", "failed  apb version"),
    ] {
        let server = MockServer::start().await;
        let s = Setup::new(&server.uri());
        mount_projects(&server, s.projects()).await;
        mount_health(&server, version).await;
        let cfg = s.config();
        let (code, out) = run(
            &["--config", cfg.to_str().unwrap(), "config", "check"],
            Path::new("/"),
            &[("NEARAI_API_KEY", "sk-synthetic-test-key")],
        )
        .await;
        assert_ne!(code, 0, "{version}: {out}");
        assert!(out.contains(want), "{version}: {out}");
        if version == "dev" {
            assert!(out.contains("1.0.0-beta.7 or newer"), "{out}");
        }
    }
}

#[tokio::test]
async fn config_check_unknown_key_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("nearbee.yaml"), "apb:\n  port: 1\n").unwrap();
    let (code, out) = run(&["config", "check"], dir.path(), &[]).await;
    assert_ne!(code, 0);
    assert!(out.starts_with("failed") && out.contains("port"), "{out}");
}

#[tokio::test]
async fn config_check_refuses_credentials_in_the_apb_url_without_printing_them() {
    let s = Setup::new(DEAD_URL);
    let cfg = s.config();
    let key = "apb_synthetic_userinfo_key";
    let url = format!("http://operator:{key}@127.0.0.1:1");
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "config", "check"],
        Path::new("/"),
        &[
            ("NEARAI_API_KEY", "sk-synthetic-test-key"),
            ("APB_API_KEY", key),
            ("APB_URL", &url),
        ],
    )
    .await;
    assert_ne!(code, 0, "{out}");
    assert!(
        out.starts_with("failed") && out.contains("APB_URL"),
        "{out}"
    );
    assert!(!out.contains(key) && !out.contains("operator"), "{out}");
}

#[tokio::test]
async fn every_cli_diagnostic_line_is_redacted() {
    // A secret that reaches a diagnostic another way (here: the URL path)
    // is still removed from every line the commands print.
    let key = "apb_synthetic_path_key";
    let url = format!("{DEAD_URL}/{key}");
    let s = Setup::new(&url);
    let cfg = s.config();
    let vars = [
        ("NEARAI_API_KEY", "sk-synthetic-test-key"),
        ("APB_API_KEY", key),
    ];
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "config", "check"],
        Path::new("/"),
        &vars,
    )
    .await;
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("apb dashboard"), "{out}");
    assert!(!out.contains(key), "{out}");
    let (code, out) = run(
        &[
            "--config",
            cfg.to_str().unwrap(),
            "runs",
            "find",
            "--hire-id",
            "h1",
        ],
        Path::new("/"),
        &vars,
    )
    .await;
    assert_eq!(code, nearbee::EXIT_ERROR, "{out}");
    assert!(out.starts_with("error: apb"), "{out}");
    assert!(!out.contains(key), "{out}");
}

async fn find_setup(body: Value) -> (MockServer, Setup) {
    let server = MockServer::start().await;
    let s = Setup::new(&server.uri());
    mount_projects(&server, s.projects()).await;
    let f = fixture("find_hire");
    let term = f["request"]["query"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == "field")
        .unwrap()[1]
        .as_str()
        .unwrap()
        .to_owned();
    Mock::given(method("GET"))
        .and(path("/api/runs"))
        .and(query_param("workspace", WS))
        .and(query_param("field", term.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    (server, s)
}

#[tokio::test]
async fn runs_find_match() {
    let (_server, s) = find_setup(fixture("find_hire")["response"]["body"].clone()).await;
    let cfg = s.config();
    let (code, out) = run(
        &[
            "--config",
            cfg.to_str().unwrap(),
            "runs",
            "find",
            "--hire-id",
            "fixture-hire-737190",
        ],
        Path::new("/"),
        &[],
    )
    .await;
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "smoke-own-1794253399097\tsmoke-own\tsucceeded\ncomplete: yes\n"
    );
}

#[tokio::test]
async fn runs_find_no_match_complete() {
    let (_server, s) = find_setup(json!({"runs": [], "not_indexed": 0, "withheld": 0})).await;
    let cfg = s.config();
    let (code, out) = run(
        &[
            "--config",
            cfg.to_str().unwrap(),
            "runs",
            "find",
            "--hire-id",
            "fixture-hire-737190",
        ],
        Path::new("/"),
        &[],
    )
    .await;
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "complete: yes\n");
}

#[tokio::test]
async fn runs_find_no_match_incomplete() {
    let (_server, s) = find_setup(json!({"runs": [], "not_indexed": 3, "withheld": 0})).await;
    let cfg = s.config();
    let (code, out) = run(
        &[
            "--config",
            cfg.to_str().unwrap(),
            "runs",
            "find",
            "--hire-id",
            "fixture-hire-737190",
        ],
        Path::new("/"),
        &[],
    )
    .await;
    assert_eq!(code, 2, "{out}");
    assert!(out.starts_with("complete: no"), "{out}");
}

#[tokio::test]
async fn runs_find_by_execution_id() {
    let server = MockServer::start().await;
    let s = Setup::new(&server.uri());
    mount_projects(&server, s.projects()).await;
    let f = fixture("find_execution");
    Mock::given(method("GET"))
        .and(path("/api/runs"))
        .and(query_param("workspace", WS))
        .and(query_param("field", "execution_id:fixture-exec-737190"))
        .respond_with(ResponseTemplate::new(200).set_body_json(f["response"]["body"].clone()))
        .mount(&server)
        .await;
    let cfg = s.config();
    let (code, out) = run(
        &[
            "--config",
            cfg.to_str().unwrap(),
            "runs",
            "find",
            "--execution-id",
            "fixture-exec-737190",
        ],
        Path::new("/"),
        &[],
    )
    .await;
    assert_eq!(code, 0, "{out}");
    assert!(out.ends_with("complete: yes\n"), "{out}");
}

#[test]
fn runs_find_takes_exactly_one_id() {
    assert!(Cli::try_parse_from(["nearbee", "runs", "find"]).is_err());
    assert!(
        Cli::try_parse_from([
            "nearbee",
            "runs",
            "find",
            "--hire-id",
            "a",
            "--execution-id",
            "b"
        ])
        .is_err()
    );
    assert!(Cli::try_parse_from(["nearbee", "runs", "find", "--hire-id", "a"]).is_ok());
}

#[test]
fn runs_find_rejects_empty_or_blank_ids() {
    for flag in ["--hire-id", "--execution-id", "--playbook"] {
        for value in ["", "   ", "\t"] {
            let mut args = vec!["nearbee", "runs", "find"];
            if flag == "--playbook" {
                args.extend(["--hire-id", "a"]);
            }
            args.extend([flag, value]);
            assert!(
                Cli::try_parse_from(&args).is_err(),
                "{flag} {value:?} was accepted"
            );
        }
    }
}

#[tokio::test]
async fn serve_exits_4_naming_the_lock_while_another_process_holds_it() {
    // A recording apb: the held lock is found before any call to it.
    let server = MockServer::start().await;
    let url = server.uri();
    let s = Setup::new(&url);
    let state = s.dir.path().join("state");
    std::fs::write(
        s.config(),
        format!(
            "state_dir: {}\napb:\n  url: {url}\n  workspace_path: {}\n",
            state.display(),
            s.workspace.display()
        ),
    )
    .unwrap();
    let _held = nearbee_core::lock::acquire(&state).unwrap();
    let cfg = s.config();
    let creds = s.dir.path().join("credentials.json");
    let (code, out) = run(
        &["--config", cfg.to_str().unwrap(), "serve"],
        Path::new("/"),
        &[("NEARBEE_CREDENTIALS", creds.to_str().unwrap())],
    )
    .await;
    assert_eq!(code, 4, "{out}");
    // The message wording is owned by `lock.rs`; the CLI names the path.
    let lock = nearbee_core::lock::lock_path(&state);
    assert!(out.contains(&lock.display().to_string()), "{out}");
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "apb was called before the lock check"
    );
}
