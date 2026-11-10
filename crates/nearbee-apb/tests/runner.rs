//! `ApbRunner` against recorded apb answers (`tests/fixtures/apb/`), replayed
//! with wiremock. Each mock matches method, path, query and, where the
//! runner can produce the recorded request, the body, so a wrong query or
//! body fails the test.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use nearbee_apb::{ApbRunner, ConnectError};
use nearbee_core::redact::DETAIL_LIMIT;
use nearbee_core::runner::{
    ExecutionId, HireId, PlaybookId, RunId, RunQuery, RunStatus, Runner, RunnerError, SearchField,
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn workspace_is_resolved_from_the_project_list() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    assert_eq!(runner.workspace().as_str(), WS);
}

#[tokio::test]
async fn unknown_workspace_names_the_path_and_the_known_projects() {
    let server = MockServer::start().await;
    recorded(&fixture("projects"), true).mount(&server).await;
    let err = ApbRunner::connect(opts(&server), Path::new("/nowhere/ws"))
        .await
        .unwrap_err();
    let ConnectError::Workspace(w) = err else {
        panic!("{err:?}")
    };
    let msg = w.to_string();
    assert!(msg.contains("/nowhere/ws"), "{msg}");
    assert!(msg.contains(WS_PATH), "{msg}");
    assert!(msg.contains("scratch-project"), "{msg}");
}

#[tokio::test]
async fn playbooks_of_another_workspace_are_ignored() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("playbooks");
    let rows = f["response"]["body"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|p| p["workspace_id"] == OTHER_WS && p["id"] == "smoke-own"),
        "the fixture holds the same id in another workspace"
    );
    recorded(&f, true).mount(&server).await;
    let list = runner.playbooks().await.unwrap();
    let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["smoke-deepseek", "smoke-own", "smoke-qwen"]);
    let pinned = rows.iter().filter(|p| p["workspace_id"] == WS).count();
    assert_eq!(list.len(), pinned);
    assert_eq!(list[1].version.as_deref(), Some("1.0.1"));
}

#[tokio::test]
async fn estimate_carries_the_workspace_and_reads_the_answer() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&fixture("estimate"), true).mount(&server).await;
    let e = runner
        .estimate(&PlaybookId::new("smoke-own"))
        .await
        .unwrap();
    assert_eq!(e.cost_usd, None, "no history: cost unknown, not zero");
    assert_eq!(e.tokens, None);
    assert_eq!(e.samples, 0);
    assert_eq!(e.duration_secs, Some(Decimal::from(130)));
    assert!(e.summary.unwrap().contains("smoke-own"));
}

#[tokio::test]
async fn start_sends_hire_and_execution_ids_and_returns_the_run_id() {
    // Rows: the recorded start, and a queued start (spec scenario "Queued
    // start": apb queues a start behind a busy workdir and answers 200 with
    // the run id as usual, docs: DEPLOYMENT.md "The workdir queue").
    for run_id in [None, Some("smoke-own-queued-1")] {
        let server = MockServer::start().await;
        let runner = connected(&server).await;
        let f = fixture("start");
        let (req, body, _dir) = start_request(&f, "smoke-own");
        assert!(!req.params.contains_key("hire_id"));
        let mut answer = f["response"]["body"].clone();
        if let Some(id) = run_id {
            answer["run_id"] = json!(id);
        }
        builder(&f, false)
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer))
            .expect(1)
            .mount(&server)
            .await;
        let id = runner.start(req).await.unwrap();
        assert_eq!(id.as_str(), run_id.unwrap_or("smoke-own-1794253399097"));
    }
}

/// The start body: no `budget` key when the caller passes none, and
/// `continued_from` when a continued run is requested (spec `apb-runner`,
/// Run start, scenarios "No budget" and "Continued run").
#[tokio::test]
async fn start_without_budget_omits_the_key_and_continued_from_travels() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("start");
    // Without a budget: the body carries no `budget` key at all.
    let (mut req, mut body, _dir) = start_request(&f, "smoke-own");
    req.budget = None;
    body.as_object_mut().unwrap().remove("budget");
    builder(&f, false)
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": "smoke-own-nb"})))
        .expect(1)
        .mount(&server)
        .await;
    let id = runner.start(req).await.unwrap();
    assert_eq!(id.as_str(), "smoke-own-nb");
    // A continued run names the run it continues.
    let (mut req2, mut body2, _dir2) = start_request(&f, "smoke-own");
    req2.continued_from = Some(RunId::new("run-prev"));
    body2["continued_from"] = json!("run-prev");
    builder(&f, false)
        .and(body_json(body2))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": "smoke-own-cf"})))
        .expect(1)
        .mount(&server)
        .await;
    let id = runner.start(req2).await.unwrap();
    assert_eq!(id.as_str(), "smoke-own-cf");
}

/// A partial cost is incomplete, the split by basis travels, and a
/// budget pause is reported (spec `apb-runner`, Run observation; missing
/// cost is `missing_cost_is_unknown_not_zero`). The two fixtures are
/// synthetic, built from the apb source types.
#[tokio::test]
async fn usage_is_parsed_from_the_apb_answer_shapes() {
    // A terminal run with a cost of one of three finished attempts: the
    // cost is incomplete, and the split by basis travels.
    let f = fixture("run_partial_cost");
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&f, true).mount(&server).await;
    let d = runner
        .run(&RunId::new(
            f["response"]["body"]["run_id"].as_str().unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        d.usage.cost_usd,
        Some(Decimal::from_str_exact("0.05").unwrap())
    );
    assert_eq!(d.usage.cost_complete(), Some(false));
    let basis = d.usage.cost_by_basis.expect("the split travels");
    assert_eq!(
        basis.billed_usd,
        Some(Decimal::from_str_exact("0.03").unwrap())
    );
    assert_eq!(
        basis.subscription_equivalent_usd,
        Some(Decimal::from_str_exact("0.02").unwrap())
    );
    assert_eq!(basis.unknown_basis_usd, None);
    assert_eq!(d.usage.attempts, Some(3));
    assert_eq!(d.usage.finished_attempts, Some(3));
    assert_eq!(d.usage.cost_attempts, Some(1));

    // A run the budget gate holds reads as paused by its budget.
    let f = fixture("run_budget_paused");
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&f, true).mount(&server).await;
    let d = runner
        .run(&RunId::new(
            f["response"]["body"]["run_id"].as_str().unwrap(),
        ))
        .await
        .unwrap();
    assert!(d.budget_paused, "the budget pause is reported");
    assert_eq!(d.status, RunStatus::Paused);
}

#[tokio::test]
async fn reserved_param_is_refused_before_sending() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    for name in ["hire_id", "execution_id"] {
        let (mut req, _, _dir) = start_request(&fixture("start"), "smoke-own");
        req.params.insert(name.into(), "x".into());
        let err = runner.start(req).await.unwrap_err();
        assert!(
            matches!(&err, RunnerError::Invalid { detail } if detail.contains(name)),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn missing_worktree_is_refused_before_sending() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (mut req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    req.worktree = PathBuf::from("/definitely/not/here");
    let err = runner.start(req).await.unwrap_err();
    assert!(matches!(err, RunnerError::Invalid { .. }), "{err:?}");
}

#[tokio::test]
async fn budget_refusal_keeps_code_and_estimate() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("refusal_budget");
    // The recorded request left on_no_estimate unset (that is when apb
    // refuses); the runner always sends one, so the body is not matched.
    let (req, _, _dir) = start_request(&f, "smoke-own");
    recorded(&f, false).expect(1).mount(&server).await;
    let err = runner.start(req).await.unwrap_err();
    let RunnerError::Refused {
        status,
        code,
        estimate,
        ..
    } = err
    else {
        panic!("{err:?}")
    };
    assert_eq!(status, 409);
    assert_eq!(code, "budget_needs_decision");
    assert_eq!(estimate.unwrap()["playbook"], "smoke-own");
}

#[tokio::test]
async fn launch_gate_is_refused_and_never_confirmed() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("refusal_launch");
    let (req, body, _dir) = start_request(&f, "smoke-qwen");
    // Exactly one POST: the runner never sends confirm_launch.
    builder(&f, false)
        .and(body_json(body))
        .respond_with(response(&f))
        .expect(1)
        .mount(&server)
        .await;
    let err = runner.start(req).await.unwrap_err();
    let RunnerError::Refused { status, code, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(*status, 409);
    assert_eq!(code, "billing_requires_confirmation");
    for r in server.received_requests().await.unwrap() {
        assert!(!String::from_utf8_lossy(&r.body).contains("confirm_launch"));
    }
}

#[tokio::test]
async fn plain_text_422_is_refused_with_the_text() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("refusal_invalid");
    let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    recorded(&f, false).expect(1).mount(&server).await;
    let err = runner.start(req).await.unwrap_err();
    let RunnerError::Refused { status, detail, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(*status, 422);
    assert_eq!(detail, f["response"]["body"].as_str().unwrap());
}

#[tokio::test]
async fn timeout_on_start_is_unknown_and_not_retried() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("start");
    let (req, _, _dir) = start_request(&f, "smoke-own");
    Mock::given(method("POST"))
        .respond_with(response(&f).set_delay(Duration::from_secs(3)))
        .expect(1)
        .mount(&server)
        .await;
    let err = runner.start(req).await.unwrap_err();
    assert!(
        matches!(err, RunnerError::StartOutcomeUnknown { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn server_error_or_malformed_success_on_start_is_unknown() {
    for resp in [
        ResponseTemplate::new(500).set_body_string("boom"),
        ResponseTemplate::new(200).set_body_string("not json"),
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
        // An id the runner could never observe is no known start either.
        ResponseTemplate::new(200).set_body_json(json!({"run_id": ""})),
        ResponseTemplate::new(200).set_body_json(json!({"run_id": "a/b"})),
        ResponseTemplate::new(200).set_body_json(json!({"run_id": ".."})),
    ] {
        let server = MockServer::start().await;
        let runner = connected(&server).await;
        let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
        Mock::given(method("POST"))
            .respond_with(resp)
            .expect(1)
            .mount(&server)
            .await;
        let err = runner.start(req).await.unwrap_err();
        assert!(
            matches!(err, RunnerError::StartOutcomeUnknown { .. }),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn redirect_on_start_is_unknown_not_a_protocol_error() {
    // The start was sent; a 3xx answer does not say whether apb started it.
    for status in [301, 302, 303, 307, 308] {
        let server = MockServer::start().await;
        let runner = connected(&server).await;
        let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status).insert_header("location", "http://example.invalid/x"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let err = runner.start(req).await.unwrap_err();
        assert!(
            matches!(err, RunnerError::StartOutcomeUnknown { .. }),
            "{status}: {err:?}"
        );
    }
}

#[tokio::test]
async fn unauthorized_or_forbidden_start_is_refused() {
    for status in [401u16, 403] {
        let server = MockServer::start().await;
        let runner = connected(&server).await;
        let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"error": "unauthorized"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let err = runner.start(req).await.unwrap_err();
        let RunnerError::Refused {
            status: got, code, ..
        } = &err
        else {
            panic!("{status}: {err:?}")
        };
        assert_eq!(*got, status);
        assert_eq!(code, "unauthorized");
    }
}

#[tokio::test]
async fn dropped_connection_on_start_is_unknown() {
    // The start request reaches the server, which closes the connection
    // without answering: the runner cannot tell whether apb started the run,
    // so it reports the start as unknown and never retries. The server keeps
    // its port for the whole test, so no other server can answer instead.
    let projects = fixture("projects")["response"]["body"].clone();
    let server = RawServer::start(move |line, _| {
        if line.starts_with("GET /api/projects") {
            Reply::Bytes(ok_json(&projects))
        } else {
            Reply::Drop
        }
    });
    let runner = ApbRunner::connect(raw_opts(&server), Path::new(WS_PATH))
        .await
        .unwrap();
    let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    let err = runner.start(req).await.unwrap_err();
    assert!(
        matches!(err, RunnerError::StartOutcomeUnknown { .. }),
        "{err:?}"
    );
    assert_eq!(server.count("POST /api/playbooks/smoke-own/run"), 1);
}

#[tokio::test]
async fn body_cut_short_on_a_read_is_retried() {
    // The first answer promises more body than it sends and closes: a
    // transport failure while reading the body, retried like one before the
    // headers.
    let projects = fixture("projects")["response"]["body"].clone();
    let run = fixture("run_succeeded")["response"]["body"].clone();
    let server = RawServer::start(move |line, n| {
        if line.starts_with("GET /api/projects") {
            Reply::Bytes(ok_json(&projects))
        } else if n == 0 {
            Reply::Bytes(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 5000\r\nconnection: close\r\n\r\n{\"run_id\"".to_vec(),
            )
        } else {
            Reply::Bytes(ok_json(&run))
        }
    });
    let runner = ApbRunner::connect(raw_opts(&server), Path::new(WS_PATH))
        .await
        .unwrap();
    let d = runner
        .run(&RunId::new("smoke-own-1794253399097"))
        .await
        .unwrap();
    assert_eq!(d.status, RunStatus::Succeeded);
    assert_eq!(server.count("GET /api/runs/smoke-own-1794253399097"), 2);
}

#[tokio::test]
async fn body_cut_short_on_every_try_is_a_retryable_read() {
    let projects = fixture("projects")["response"]["body"].clone();
    let server = RawServer::start(move |line, _| {
        if line.starts_with("GET /api/projects") {
            Reply::Bytes(ok_json(&projects))
        } else {
            Reply::Bytes(
                b"HTTP/1.1 200 OK\r\ncontent-length: 5000\r\nconnection: close\r\n\r\n{".to_vec(),
            )
        }
    });
    let runner = ApbRunner::connect(raw_opts(&server), Path::new(WS_PATH))
        .await
        .unwrap();
    let err = runner.run(&RunId::new("r1")).await.unwrap_err();
    assert!(matches!(err, RunnerError::RetryableRead { .. }), "{err:?}");
    assert_eq!(
        server.count("GET /api/runs/r1"),
        4,
        "one try and three retries"
    );
}

#[tokio::test]
async fn not_found_whose_body_read_fails_is_not_found_and_sent_once() {
    // A 404 that promises more body than it sends: the status already says
    // the run is unknown, so the read is not retried.
    let projects = fixture("projects")["response"]["body"].clone();
    let server = RawServer::start(move |line, _| {
        if line.starts_with("GET /api/projects") {
            Reply::Bytes(ok_json(&projects))
        } else {
            Reply::Bytes(
                b"HTTP/1.1 404 Not Found\r\ncontent-length: 5000\r\nconnection: close\r\n\r\n{"
                    .to_vec(),
            )
        }
    });
    let runner = ApbRunner::connect(raw_opts(&server), Path::new(WS_PATH))
        .await
        .unwrap();
    let err = runner.run(&RunId::new("r1")).await.unwrap_err();
    match &err {
        RunnerError::NotFound { detail } => {
            assert_eq!(detail, "GET /api/runs/r1: HTTP 404", "{err:?}")
        }
        _ => panic!("expected NotFound, got {err:?}"),
    }
    assert_eq!(server.count("GET /api/runs/r1"), 1, "sent once");
}

#[tokio::test]
async fn ids_that_are_not_one_path_segment_are_refused_before_sending() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    // Nothing but the project list may reach the server.
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    for bad in ["", ".", "..", "a/b", "../x", "a%2Fb", "a b", "a?b"] {
        let err = runner.run(&RunId::new(bad)).await.unwrap_err();
        assert!(
            matches!(err, RunnerError::Invalid { .. }),
            "run {bad:?}: {err:?}"
        );
        let err = runner.estimate(&PlaybookId::new(bad)).await.unwrap_err();
        assert!(
            matches!(err, RunnerError::Invalid { .. }),
            "estimate {bad:?}: {err:?}"
        );
        let (mut req, _, _dir) = start_request(&fixture("start"), "smoke-own");
        req.playbook = PlaybookId::new(bad);
        let err = runner.start(req).await.unwrap_err();
        assert!(
            matches!(err, RunnerError::Invalid { .. }),
            "start {bad:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn refusal_code_detail_and_estimate_are_bounded() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    let long = "x".repeat(10_000);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error": long,
            "detail": "d".repeat(10_000),
            "estimate": {"note": "y".repeat(20_000)},
        })))
        .expect(1)
        .mount(&server)
        .await;
    let err = runner.start(req).await.unwrap_err();
    let RunnerError::Refused {
        code,
        detail,
        estimate,
        ..
    } = &err
    else {
        panic!("{err:?}")
    };
    assert!(code.len() <= 128, "code is {} bytes", code.len());
    assert!(
        detail.len() <= DETAIL_LIMIT,
        "detail is {} bytes",
        detail.len()
    );
    // Every string inside the estimate is cut like a detail, so one long
    // string leaves a kept estimate within 8 KiB.
    let e = estimate
        .as_ref()
        .expect("an estimate with one long string is kept, cut");
    assert!(
        e.to_string().len() <= 8192,
        "estimate is {} bytes",
        e.to_string().len()
    );
    assert!(e["note"].as_str().unwrap().len() <= DETAIL_LIMIT);

    // An estimate still larger than 8 KiB after the cut is dropped, not cut.
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    let many: serde_json::Map<String, Value> = (0..10)
        .map(|i| (format!("k{i}"), json!("z".repeat(2_000))))
        .collect();
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error": "budget_needs_decision",
            "estimate": many,
        })))
        .expect(1)
        .mount(&server)
        .await;
    let err = runner.start(req).await.unwrap_err();
    let RunnerError::Refused { estimate, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(
        estimate, &None,
        "a 20 KB estimate of short strings is dropped"
    );

    // A plain-text refusal detail is bounded too.
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_string("t".repeat(10_000)))
        .expect(1)
        .mount(&server)
        .await;
    let err = runner.start(req).await.unwrap_err();
    let RunnerError::Refused { detail, .. } = &err else {
        panic!("{err:?}")
    };
    assert!(
        detail.len() <= DETAIL_LIMIT,
        "detail is {} bytes",
        detail.len()
    );
}

#[tokio::test]
async fn retryable_read_is_retried_three_times_then_fails() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/runs/r1"))
        .respond_with(ResponseTemplate::new(503))
        .expect(4)
        .mount(&server)
        .await;
    let err = runner.run(&RunId::new("r1")).await.unwrap_err();
    assert!(matches!(err, RunnerError::RetryableRead { .. }), "{err:?}");
}

#[tokio::test]
async fn read_recovers_after_a_retry() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("run_succeeded");
    Mock::given(method("GET"))
        .and(path("/api/runs/smoke-own-1794253399097"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    recorded(&f, true).mount(&server).await;
    let d = runner
        .run(&RunId::new("smoke-own-1794253399097"))
        .await
        .unwrap();
    assert_eq!(d.status, RunStatus::Succeeded);
}

#[tokio::test]
async fn run_404_is_not_found() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&fixture("run_404"), true)
        .expect(1)
        .mount(&server)
        .await;
    let err = runner
        .run(&RunId::new("run-does-not-exist"))
        .await
        .unwrap_err();
    assert!(matches!(err, RunnerError::NotFound { .. }), "{err:?}");
}

#[tokio::test]
async fn running_and_succeeded_runs_are_read() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&fixture("run_running"), true)
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let id = RunId::new("smoke-own-1794253399097");
    let d = runner.run(&id).await.unwrap();
    assert_eq!(d.status, RunStatus::Running);
    assert!(!d.status.is_terminal());

    recorded(&fixture("run_succeeded"), true)
        .mount(&server)
        .await;
    let d = runner.run(&id).await.unwrap();
    assert_eq!(d.status, RunStatus::Succeeded);
    assert!(d.status.is_terminal() && d.status.is_success());
    assert_eq!(d.playbook.as_str(), "smoke-own");
    assert_eq!(d.playbook_version.as_deref(), Some("1.0.1"));
    assert_eq!(
        d.worktree.as_deref(),
        Some(Path::new(
            "<home>/.local/share/nearbee/apb-workspace/work/smoke-737190"
        ))
    );
    assert_eq!(d.params["hire_id"], "fixture-hire-737190");
    assert_eq!(d.usage.cost_usd, Some(Decimal::new(250714, 8)));
    assert_eq!(d.usage.output_tokens, Some(557));
    let manifest = d.nodes.iter().find(|n| n.id == "manifest").unwrap();
    assert_eq!(manifest.status, "succeeded");
    assert!(
        manifest
            .outputs
            .as_ref()
            .unwrap()
            .as_str()
            .unwrap()
            .contains("sha256")
    );
}

async fn run_with(body: Value) -> nearbee_core::runner::RunDetail {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("run_succeeded");
    recorded_with(&f, true, ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    runner
        .run(&RunId::new("smoke-own-1794253399097"))
        .await
        .unwrap()
}

#[tokio::test]
async fn run_status_is_read_from_run_status() {
    // The mapping of every status is owned by `RunStatus::parse` (core unit
    // tests); this proves the client reads the `run_status` field.
    let mut body = fixture("run_succeeded")["response"]["body"].clone();
    body["run_status"] = json!("paused");
    assert_eq!(run_with(body).await.status, RunStatus::Paused);
}

#[tokio::test]
async fn unknown_status_is_neither_success_nor_terminal() {
    let mut body = fixture("run_succeeded")["response"]["body"].clone();
    body["run_status"] = json!("teleported");
    let d = run_with(body).await;
    assert_eq!(d.status, RunStatus::Unknown("teleported".into()));
    assert!(!d.status.is_terminal() && !d.status.is_success());
}

#[tokio::test]
async fn missing_cost_is_unknown_not_zero() {
    let mut body = fixture("run_succeeded")["response"]["body"].clone();
    body["usage"].as_object_mut().unwrap().remove("cost_usd");
    assert_eq!(run_with(body.clone()).await.usage.cost_usd, None);
    body["usage"] = Value::Null;
    let d = run_with(body).await;
    assert_eq!(d.usage.cost_usd, None);
    assert_eq!(d.usage.output_tokens, None);
}

#[tokio::test]
async fn find_by_hire_and_execution_id_is_complete() {
    let server = MockServer::start().await;
    // The project list is read once, at connect, however many calls follow.
    recorded(&fixture("projects"), true)
        .expect(1)
        .mount(&server)
        .await;
    let runner = ApbRunner::connect(opts(&server), Path::new(WS_PATH))
        .await
        .unwrap();
    recorded(&fixture("find_hire"), true).mount(&server).await;
    recorded(&fixture("find_execution"), true)
        .mount(&server)
        .await;
    for field in [
        SearchField::HireId(HireId::new("fixture-hire-737190")),
        SearchField::ExecutionId(ExecutionId::new("fixture-exec-737190")),
    ] {
        let l = runner
            .find(RunQuery {
                playbook: Some(PlaybookId::new("smoke-own")),
                field,
            })
            .await
            .unwrap();
        assert!(l.complete);
        assert_eq!(l.runs.len(), 1);
        assert_eq!(l.runs[0].id.as_str(), "smoke-own-1794253399097");
        assert_eq!(l.runs[0].status, RunStatus::Succeeded);
    }
}

async fn lookup_with(
    not_indexed: u64,
    withheld: u64,
    runs: bool,
) -> nearbee_core::runner::RunLookup {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("find_hire");
    let mut body = f["response"]["body"].clone();
    body["not_indexed"] = json!(not_indexed);
    body["withheld"] = json!(withheld);
    if !runs {
        body["runs"] = json!([]);
    }
    recorded_with(&f, true, ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    runner
        .find(RunQuery {
            playbook: Some(PlaybookId::new("smoke-own")),
            field: SearchField::HireId(HireId::new("fixture-hire-737190")),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn withheld_or_unindexed_runs_make_the_lookup_incomplete() {
    let l = lookup_with(0, 2, true).await;
    assert!(!l.complete, "incomplete even with a match");
    assert_eq!(l.runs.len(), 1);
    let l = lookup_with(5, 0, false).await;
    assert!(!l.complete);
    assert_eq!(l.not_indexed, 5);
    let l = lookup_with(0, 0, false).await;
    assert!(l.complete && l.runs.is_empty());
}

#[tokio::test]
async fn no_credential_is_sent_without_a_key() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&fixture("run_succeeded"), true)
        .mount(&server)
        .await;
    let f = fixture("start");
    builder(&f, false)
        .respond_with(response(&f))
        .mount(&server)
        .await;
    runner
        .run(&RunId::new("smoke-own-1794253399097"))
        .await
        .unwrap();
    let (req, _, _dir) = start_request(&f, "smoke-own");
    runner.start(req).await.unwrap();
    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 3, "projects, run, start");
    for r in received {
        assert!(
            r.headers.get("authorization").is_none(),
            "{} {}",
            r.method,
            r.url
        );
    }
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/runs/r1"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "http://example.invalid/x"),
        )
        .mount(&server)
        .await;
    let err = runner.run(&RunId::new("r1")).await.unwrap_err();
    assert!(matches!(err, RunnerError::Protocol { .. }), "{err:?}");
}

#[test]
fn fixtures_hold_no_key_and_no_home_path() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/apb");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/nonexistent-home".into());
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains(&home), "{} holds the home path", p.display());
        assert!(
            !text.contains("/home/"),
            "{} holds a home path",
            p.display()
        );
        assert!(!text.to_lowercase().contains("bearer "), "{}", p.display());
        let re_key = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|w| w.starts_with("apb_") && w.len() > 20);
        assert!(!re_key, "{} holds a key-shaped value", p.display());
    }
}
