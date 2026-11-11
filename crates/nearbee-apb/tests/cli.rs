//! `Runner::abort` and `Runner::decide` against a fake `apb` CLI script
//! that prints the real wording of apb 1.0.0-beta.6 (`crates/apb-cli/src/
//! run.rs` `stop_cmd`, `crates/apb-engine/src/decision/host_task.rs`
//! `AskOutcome::to_json`). Every call's argv and environment are logged.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use nearbee_apb::{ApbOptions, ApbRunner, Timing};
use nearbee_core::runner::{DecideOutcome, Runner, RunnerError};
use rust_decimal::Decimal;
use wiremock::MockServer;

/// The fake: mode-driven answers, the argv of every call and the
/// environment it saw logged into its state directory.
const FAKE: &str = r#"#!/bin/sh
STATE="${APB_CONFIG_DIR:?}"
printf '%s\n' "$*" >> "$STATE/argv.log"
i=0
for a in "$@"; do
  printf '%s' "$a" > "$STATE/arg.$i"
  i=$((i + 1))
done
echo "$#" > "$STATE/argc"
env | sort > "$STATE/env.log"
case "$(cat "$STATE/mode" 2>/dev/null)" in
  stop-ok)
    [ "$1" = "stop" ] || { echo "fake: expected stop, got $*" >&2; exit 2; }
    echo "stopping $2: abort sent to the running driver"
    exit 0
    ;;
  stop-finished)
    echo "$2 had already finished, so no run was stopped"
    exit 0
    ;;
  stop-fail)
    echo "stop failed: run $2 is unknown" >&2
    exit 2
    ;;
  sleep)
    sleep 2
    echo late
    exit 0
    ;;
  decide-yes)
    printf '%s\n' '{"kind":"is","answer":"yes","p":0.9,"provider":"openrouter","model":"m","calibrated":true,"mode":"fast","cached":false,"answered":true}'
    exit 0
    ;;
  decide-no-05)
    printf '%s\n' '{"kind":"is","answer":"no","p":0.05,"provider":"openrouter","model":"m","calibrated":true,"mode":"fast","cached":false,"answered":true}'
    exit 0
    ;;
  decide-no-35)
    printf '%s\n' '{"kind":"is","answer":"no","p":0.35,"provider":"openrouter","model":"m","calibrated":true,"mode":"fast","cached":false,"answered":true}'
    exit 0
    ;;
  decide-refused)
    printf '%s\n' '{"answered":false,"refused":"no_provider","reason":"no decision provider is configured"}'
    exit 1
    ;;
  decide-error)
    printf '%s\n' '{"answered":false,"error":"boom","reason":"the decision providers did not answer (boom); decide without them"}'
    exit 1
    ;;
  *)
    echo "fake: unknown mode $(cat "$STATE/mode" 2>/dev/null)" >&2
    exit 2
    ;;
esac
"#;

/// A runner whose CLI is the fake. The recorded project list names the
/// workspace under a masked `<home>`; the mask is rewritten to the temp
/// directory and that project path (created for real) becomes the
/// workspace, so the CLI runs with a working `current_dir`.
async fn cli_runner(state: &Path, ws: &Path) -> ApbRunner {
    let server = MockServer::start().await;
    let mut f = fixture("projects");
    let home = ws
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap()
        .display()
        .to_string();
    for p in f["response"]["body"].as_array_mut().unwrap() {
        let masked = p["path"].as_str().unwrap().to_owned();
        p["path"] = serde_json::Value::from(masked.replace("<home>", &home));
    }
    recorded(&f, true).mount(&server).await;
    let bin = state.parent().unwrap().join("fake-apb.sh");
    std::fs::write(&bin, FAKE).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    ApbRunner::connect(
        ApbOptions {
            url: server.uri(),
            api_key: None,
            redact: vec![],
            timing: Timing {
                abort: Duration::from_millis(500),
                decide: Duration::from_millis(500),
                ..fast_timing()
            },
            bin,
            config_dir: state.to_path_buf(),
            systemctl_bin: std::path::PathBuf::from("systemctl"),
            xdg_config_home: std::path::PathBuf::from("/nonexistent/xdg-config"),
        },
        ws,
    )
    .await
    .unwrap()
}

fn harness() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("apb-config");
    std::fs::create_dir_all(&state).unwrap();
    // The workspace as the projects fixture names it, under the temp home.
    let ws = dir.path().join(".local/share/nearbee/apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();
    (dir, state, ws)
}

fn set_mode(state: &Path, mode: &str) {
    std::fs::write(state.join("mode"), mode).unwrap();
}

fn argv_of(state: &Path) -> Vec<String> {
    std::fs::read_to_string(state.join("argv.log"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Abort: exit 0 is success, also for a run that had already finished
/// (apb's "had already finished" wording).
#[tokio::test]
async fn abort_reports_success_on_exit_zero() {
    for mode in ["stop-ok", "stop-finished"] {
        let (_dir, state, ws) = harness();
        set_mode(&state, mode);
        let runner = cli_runner(&state, &ws).await;
        runner.abort(&RunId::new("run-1")).await.unwrap();
        let argv = argv_of(&state);
        assert_eq!(
            argv.last().map(String::as_str),
            Some("stop run-1"),
            "{mode}"
        );
        // The call runs in the workspace with the instance's config dir.
        let env = std::fs::read_to_string(state.join("env.log")).unwrap();
        assert!(
            env.contains(&format!("APB_CONFIG_DIR={}", state.display())),
            "{mode}: {env}"
        );
    }
}

/// Abort: a non-zero exit is a refusal with apb's own wording.
#[tokio::test]
async fn abort_reports_a_non_zero_exit() {
    let (_dir, state, ws) = harness();
    set_mode(&state, "stop-fail");
    let runner = cli_runner(&state, &ws).await;
    let err = runner.abort(&RunId::new("run-1")).await.unwrap_err();
    match err {
        RunnerError::Refused { detail, .. } => {
            assert!(detail.contains("stop failed"), "{detail}");
            assert!(detail.contains("run run-1 is unknown"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

/// Abort: a hung child is killed at the timeout.
#[tokio::test]
async fn abort_kills_a_hung_child() {
    let (_dir, state, ws) = harness();
    set_mode(&state, "sleep");
    let runner = cli_runner(&state, &ws).await;
    let err = runner.abort(&RunId::new("run-1")).await.unwrap_err();
    assert!(err.to_string().contains("did not finish within"), "{err}");
}

fn is_answer(yes: bool, p: Option<Decimal>) -> DecideOutcome {
    DecideOutcome::Answered { yes, p }
}

/// Decide: answered yes and no, with the probability of yes (spec
/// `apb-runner`, Decide).
#[tokio::test]
async fn decide_reads_answers_with_their_probability() {
    let cases = [
        ("decide-yes", true, Some(Decimal::new(9, 1))),
        ("decide-no-05", false, Some(Decimal::new(5, 2))),
        ("decide-no-35", false, Some(Decimal::new(35, 2))),
    ];
    for (mode, yes, p) in cases {
        let (_dir, state, ws) = harness();
        set_mode(&state, mode);
        let runner = cli_runner(&state, &ws).await;
        let out = runner
            .decide(
                "is this a request for this service",
                "yes when it asks for it",
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        assert_eq!(out, is_answer(yes, p), "{mode}");
        let argv = argv_of(&state);
        let last = argv.last().unwrap().as_str();
        assert!(
            last.starts_with("decide is ")
                && last.contains(" --criteria ")
                && last.ends_with(" --no-run"),
            "{mode}: {last}"
        );
    }
}

/// Decide: apb's refusal code and a provider error come back as
/// unanswered, with the reason (the exact line measured on Nearbee's
/// instance for `no_provider`).
#[tokio::test]
async fn decide_reads_refusals_and_errors_as_unanswered() {
    for (mode, reason) in [("decide-refused", "no_provider"), ("decide-error", "error")] {
        let (_dir, state, ws) = harness();
        set_mode(&state, mode);
        let runner = cli_runner(&state, &ws).await;
        let out = runner
            .decide("q", "c", Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(
            out,
            DecideOutcome::Unanswered {
                reason: reason.to_owned()
            },
            "{mode}"
        );
    }
}

/// Decide: a caller's limit shorter than the CLI timeout kills the child
/// at that limit (R3-9): a scope check cut off at another hire's wall
/// clock leaves no decision process running behind it.
#[tokio::test]
async fn decide_kills_the_child_at_the_callers_limit() {
    let (_dir, state, ws) = harness();
    set_mode(&state, "sleep");
    let runner = cli_runner(&state, &ws).await;
    let started = std::time::Instant::now();
    let err = runner
        .decide("q", "c", Duration::from_millis(100))
        .await
        .unwrap_err();
    let took = started.elapsed();
    assert!(err.to_string().contains("did not finish within"), "{err}");
    assert!(
        took < Duration::from_millis(400),
        "the child outlived the caller's limit: {took:?}"
    );
}

/// Decide: a brief with `$(...)` and quotes arrives as one argument; no
/// shell interprets it, and nothing in it runs.
#[tokio::test]
async fn the_brief_arrives_as_one_argument_without_a_shell() {
    let (_dir, state, ws) = harness();
    set_mode(&state, "decide-yes");
    let runner = cli_runner(&state, &ws).await;
    let brief = "is $(rm -rf ~) \"quoted\" 'single' a request?\nsecond line";
    runner
        .decide(
            brief,
            "yes when the request asks for this service, no when it clearly asks for something else",
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    // One `decide is <question> --criteria <criteria> --no-run` call, and
    // the question arrives as the one third argument, byte for byte: a
    // shell would have split it (and run the `$(...)`).
    assert_eq!(
        std::fs::read_to_string(state.join("argc")).unwrap().trim(),
        "6"
    );
    assert_eq!(
        std::fs::read_to_string(state.join("arg.0")).unwrap(),
        "decide"
    );
    assert_eq!(std::fs::read_to_string(state.join("arg.1")).unwrap(), "is");
    assert_eq!(std::fs::read_to_string(state.join("arg.2")).unwrap(), brief);
    assert_eq!(
        std::fs::read_to_string(state.join("arg.3")).unwrap(),
        "--criteria"
    );
    assert_eq!(
        std::fs::read_to_string(state.join("arg.4")).unwrap(),
        "yes when the request asks for this service, no when it clearly asks for something else"
    );
    assert_eq!(
        std::fs::read_to_string(state.join("arg.5")).unwrap(),
        "--no-run"
    );
}

/// Decide: a parent that carries `APB_RUN_ID` and `APB_NODE_ID` (serve
/// started from inside an apb run) hands neither to the child. The test
/// binary re-runs itself with both set, for the one probe below.
#[test]
fn decide_strips_the_run_and_node_ids_of_its_parent() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "decide_env_probe",
            "--ignored",
            "--test-threads=1",
        ])
        .env("APB_RUN_ID", "run-parent")
        .env("APB_NODE_ID", "node-parent")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("1 passed"), "the probe did not run: {text}");
}

/// The probe of the test above; run only through it.
#[tokio::test]
#[ignore = "run by decide_strips_the_run_and_node_ids_of_its_parent with the parent env set"]
async fn decide_env_probe() {
    assert_eq!(std::env::var("APB_RUN_ID").as_deref(), Ok("run-parent"));
    let (_dir, state, ws) = harness();
    set_mode(&state, "decide-yes");
    let runner = cli_runner(&state, &ws).await;
    runner
        .decide("is this a request?", "c", Duration::from_secs(60))
        .await
        .unwrap();
    let env = std::fs::read_to_string(state.join("env.log")).unwrap();
    assert!(!env.contains("APB_RUN_ID="), "{env}");
    assert!(!env.contains("APB_NODE_ID="), "{env}");
}

use nearbee_core::runner::RunId;
