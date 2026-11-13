//! Live smoke runs through `ApbRunner` against Nearbee's own apb instance.
//! Ignored by default and never run in CI:
//!
//! ```text
//! NEARBEE_LIVE=1 cargo test -p nearbee-apb --test live -- --ignored live_smoke_own --nocapture
//! NEARBEE_LIVE=1 cargo test -p nearbee-apb --test live -- --ignored live_smoke_qwen --nocapture
//! NEARBEE_LIVE=1 cargo test -p nearbee-apb --test live -- --ignored live_smoke_deepseek --nocapture
//! ```
//!
//! `live_smoke_own` runs on the owner's own subscription (no NEAR AI Cloud
//! credit). The `qwen` and `deepseek` runs are billed on NEAR AI Cloud
//! (owner item O-3) with a USD 0.50 budget each, and need Nearbee's apb
//! instance to allow billed work (`scripts/apb-instance.sh allow-billed`):
//! the runner never confirms a launch gate itself.
//!
//! Environment: `APB_URL` (default `http://127.0.0.1:7322`),
//! `NEARBEE_WORKSPACE` (default `~/.local/share/nearbee/apb-workspace`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nearbee_apb::{ApbOptions, ApbRunner, Timing};
use nearbee_core::config::Limits;
use nearbee_core::manifest;
use nearbee_core::runner::{
    Budget, ExecutionId, HireId, OnNoEstimate, PlaybookId, RunQuery, Runner, SearchField,
    StartRequest,
};
use rust_decimal::Decimal;

fn enabled() -> bool {
    std::env::var("NEARBEE_LIVE").as_deref() == Ok("1")
}

fn workspace() -> PathBuf {
    std::env::var("NEARBEE_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(&std::env::var("HOME").unwrap()).join(".local/share/nearbee/apb-workspace")
        })
}

async fn smoke(playbook: &str) {
    // These tests are ignored by default; run with --ignored they must run
    // for real, never report a pass without a live run.
    assert!(
        enabled(),
        "live test run without NEARBEE_LIVE=1: set it (and start Nearbee's apb instance) or do not pass --ignored"
    );
    let url = std::env::var("APB_URL").unwrap_or_else(|_| "http://127.0.0.1:7322".into());
    assert!(!url.contains(":7321"), "never the operator's apb");
    let ws = workspace();
    let runner = ApbRunner::connect(
        ApbOptions {
            url,
            api_key: None,
            redact: vec![],
            timing: Timing::default(),
            bin: std::env::var("APB_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("apb")),
            config_dir: std::env::var("APB_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    PathBuf::from(std::env::var("HOME").unwrap())
                        .join(".local/share/nearbee/apb-config")
                }),
            systemctl_bin: PathBuf::from("systemctl"),
            xdg_config_home: PathBuf::from(std::env::var("HOME").unwrap())
                .join(".local/share/nearbee/xdg-config"),
        },
        &ws,
    )
    .await
    .unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let hire = format!("live-hire-{stamp}");
    let exec = format!("live-exec-{stamp}");
    let worktree = ws.join("work").join(format!("{playbook}-{stamp}"));
    std::fs::create_dir_all(&worktree).unwrap();
    let id = runner
        .start(StartRequest {
            playbook: PlaybookId::new(playbook),
            instruction: "Synthetic Nearbee smoke run; no buyer data.".into(),
            params: BTreeMap::from([("topic".into(), "honey bees".into())]),
            worktree: worktree.clone(),
            hire_id: HireId::new(&hire),
            execution_id: ExecutionId::new(&exec),
            budget: Some(Budget {
                usd: Some(Decimal::new(50, 2)),
                on_no_estimate: OnNoEstimate::Continue,
            }),
            continued_from: None,
            direct_models: Vec::new(),
            script_models: Vec::new(),
            // No buyer data: not an external-data run.
            external_data: false,
        })
        .await
        .unwrap();
    eprintln!("run {id} started (hire {hire}, execution {exec})");
    let deadline = Instant::now() + Duration::from_secs(20 * 60);
    let detail = loop {
        let d = runner.run(&id).await.unwrap();
        if d.status.is_terminal() {
            break d;
        }
        assert!(Instant::now() < deadline, "run {id} did not end in time");
        tokio::time::sleep(Duration::from_secs(5)).await;
    };
    eprintln!(
        "run {id}: status {}, playbook {} {}, usage in {:?} out {:?} cost {}",
        detail.status,
        detail.playbook,
        detail.playbook_version.as_deref().unwrap_or("?"),
        detail.usage.input_tokens,
        detail.usage.output_tokens,
        detail
            .usage
            .cost_usd
            .map_or("not reported".to_string(), |c| format!("USD {c}")),
    );
    for n in &detail.nodes {
        eprintln!("  node {}: {}", n.id, n.status);
    }
    let found = runner
        .find(RunQuery {
            playbook: Some(PlaybookId::new(playbook)),
            field: SearchField::ExecutionId(ExecutionId::new(&exec)),
        })
        .await
        .unwrap();
    assert!(found.complete, "{found:?}");
    assert_eq!(found.runs.len(), 1, "{found:?}");
    assert_eq!(found.runs[0].id, id);
    assert!(
        detail.status.is_success(),
        "run {id} ended {}",
        detail.status
    );
    let v = manifest::validate(&worktree, &Limits::default()).unwrap();
    eprintln!(
        "manifest: {} {} bytes sha256 {}",
        v.markdown.path,
        v.markdown.bytes.len(),
        v.markdown.sha256
    );
    eprintln!("---\n{}---", v.markdown_text);
}

#[tokio::test]
#[ignore = "live: needs Nearbee's apb instance (NEARBEE_LIVE=1)"]
async fn live_smoke_own() {
    smoke("smoke-own").await;
}

#[tokio::test]
#[ignore = "live and billed on NEAR AI Cloud (owner item O-3, NEARBEE_LIVE=1)"]
async fn live_smoke_qwen() {
    smoke("smoke-qwen").await;
}

#[tokio::test]
#[ignore = "live and billed on NEAR AI Cloud (owner item O-3, NEARBEE_LIVE=1)"]
async fn live_smoke_deepseek() {
    smoke("smoke-deepseek").await;
}

/// The publish-side install (import, approval, export read-back) through
/// the apb CLI that `APB_BIN` names, against Nearbee's instance; with
/// `APB_BIN` set to the absolute path of `scripts/apb-sandboxed` it checks
/// the sandboxed instance (change apb-sandbox). No run starts, nothing is
/// billed. Environment: `APB_BIN`, `APB_CONFIG_DIR`, `NEARBEE_WORKSPACE`.
///
/// ```text
/// NEARBEE_LIVE=1 APB_BIN=$PWD/scripts/apb-sandboxed cargo test -p nearbee-apb --test live -- --ignored live_publish_install
/// ```
#[test]
#[ignore = "live: needs Nearbee's apb instance"]
fn live_publish_install() {
    use nearbee_apb::install::{ApbCli, Script, install_source};
    assert!(
        enabled(),
        "live test run without NEARBEE_LIVE=1: set it (and start Nearbee's apb instance) or do not pass --ignored"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playbooks/repo-audit");
    let source = std::fs::read_to_string(root.join("playbook.fast.yaml")).unwrap();
    let mut scripts = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(root.join("scripts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    for p in names {
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        scripts.push(Script {
            name: p.file_name().unwrap().to_string_lossy().into_owned(),
            bytes: std::fs::read(&p).unwrap(),
            executable,
        });
    }
    let cli = ApbCli {
        bin: std::env::var("APB_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("apb")),
        workspace: workspace(),
        config_dir: std::env::var("APB_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap())
                    .join(".local/share/nearbee/apb-config")
            }),
    };
    let installed = install_source(&cli, &source, "live-install-check", None, &scripts).unwrap();
    assert_eq!(installed.playbook_id, "live-install-check");
    eprintln!("installed {} {}", installed.playbook_id, installed.version);
}
