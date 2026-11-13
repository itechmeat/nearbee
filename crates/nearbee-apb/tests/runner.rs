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
    AttemptModel, ExecutionId, HireId, PlaybookId, Reported, RunId, RunQuery, RunStatus, Runner,
    RunnerError, SearchField,
};
use nearbee_core::tiers::{Known, NEARAI_BASE_URL, ProfileRef};
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
    for name in ["hire_id", "execution_id", "direct_models"] {
        let (mut req, _, _dir) = start_request(&fixture("start"), "smoke-own");
        req.params.insert(name.into(), "x".into());
        let err = runner.start(req).await.unwrap_err();
        assert!(
            matches!(&err, RunnerError::Invalid { detail } if detail.contains(name)),
            "{err:?}"
        );
    }
}

/// The frozen direct models travel as the `direct_models` param, compact
/// JSON in order; none, no param (task 3.1).
#[test]
fn sent_params_carry_the_direct_models() {
    let (mut req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    assert!(!nearbee_apb::sent_params(&req).contains_key("direct_models"));
    req.direct_models = vec![
        "Qwen/Qwen3-VL-30B-A3B-Instruct".into(),
        "openai/privacy-filter".into(),
    ];
    assert_eq!(
        nearbee_apb::sent_params(&req)["direct_models"],
        r#"["Qwen/Qwen3-VL-30B-A3B-Instruct","openai/privacy-filter"]"#
    );
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

// --- Inspection (spec `apb-runner`, Playbook inspection) ---

/// The `XDG_CONFIG_HOME` the test runners expect (`common::opts`); it does
/// not exist, so it holds no OpenCode configuration.
const TEST_XDG: &str = "/nonexistent/xdg-config";

/// Connects a runner pinned to a real workspace directory (the inspection
/// reads the workspace files), with the given fake `systemctl`, else one
/// that reports the expected `XDG_CONFIG_HOME` and nothing else.
async fn connected_at(server: &MockServer, ws: &Path, systemctl: Option<&Path>) -> ApbRunner {
    connected_xdg(server, ws, systemctl, Path::new(TEST_XDG)).await
}

/// [`connected_at`] with the expected `XDG_CONFIG_HOME` set to `xdg`.
async fn connected_xdg(
    server: &MockServer,
    ws: &Path,
    systemctl: Option<&Path>,
    xdg: &Path,
) -> ApbRunner {
    let default_sys;
    let systemctl = match systemctl {
        Some(p) => p,
        None => {
            default_sys = self::systemctl(
                ws,
                &format!("Environment=XDG_CONFIG_HOME={}", xdg.display()),
            );
            default_sys.as_path()
        }
    };
    let mut projects = fixture("projects");
    for row in projects["response"]["body"].as_array_mut().unwrap() {
        if row["path"].as_str() == Some(WS_PATH) {
            row["path"] = json!(ws.to_string_lossy());
        }
    }
    recorded(&projects, false).mount(server).await;
    let mut o = opts(server);
    o.systemctl_bin = systemctl.to_path_buf();
    o.xdg_config_home = xdg.to_path_buf();
    ApbRunner::connect(o, ws).await.unwrap()
}

/// A fake `systemctl` that prints one line; an empty body makes it exit 1.
fn systemctl(dir: &Path, line: &str) -> PathBuf {
    let p = dir.join("systemctl");
    let script = if line.is_empty() {
        "#!/bin/sh\nexit 1\n".to_owned()
    } else {
        format!("#!/bin/sh\nprintf '%s\\n' '{line}'\n")
    };
    std::fs::write(&p, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A workspace directory with an `opencode.json` and a `.apb/config.yaml`.
fn workspace_with(opencode: Option<&str>, decisions: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    if let Some(text) = opencode {
        std::fs::write(dir.path().join("opencode.json"), text).unwrap();
    }
    if let Some(text) = decisions {
        std::fs::create_dir_all(dir.path().join(".apb")).unwrap();
        std::fs::write(dir.path().join(".apb").join("config.yaml"), text).unwrap();
    }
    dir
}

const OPENCODE_JSON: &str = r#"{"provider": {
  "nearai": {"options": {"baseURL": "https://cloud-api.near.ai/v1", "apiKey": "env:NEARAI_API_KEY"}},
  "openrouter": {"options": {"baseURL": "https://openrouter.ai/api/v1", "apiKey": "env:OPENROUTER_KEY"}}
}}"#;

async fn mock_profile(server: &MockServer, name: &str, scope: &str, resp: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(format!("/api/profiles/{name}")))
        .and(wiremock::matchers::query_param("workspace", WS))
        .and(wiremock::matchers::query_param("scope", scope))
        .respond_with(resp)
        .mount(server)
        .await;
}

fn profile_answer(name: &str, scope: &str, yaml: &str, status: u16) -> ResponseTemplate {
    let body = json!({
        "bundle_digest": "sha256:0000",
        "name": name,
        "profile_digest": "sha256:0000",
        "profile_yaml": yaml,
        "scope": scope,
        "soul_md": "Do exactly the task in the prompt and nothing else.\n"
    });
    ResponseTemplate::new(status).set_body_json(body)
}

/// A small synthetic playbook source: a default profile reference and one
/// agent node, optionally with its own profile reference.
fn source_yaml(default_profile: Option<&str>, node_profile: Option<&str>) -> String {
    let mut s = String::from("schema: 2\nid: smoke-own\nversion: 1.0.0\ndefaults:\n");
    match default_profile {
        Some(p) => s.push_str(&format!("  profile: {p}\n")),
        None => s.push_str("  max_retries: null\n"),
    }
    s.push_str(
        "nodes:\n- id: start\n  type: start\n- id: write\n  type: agent_task\n  prompt: p\n",
    );
    if let Some(p) = node_profile {
        s.push_str(&format!("  profile:\n{p}\n"));
    }
    s
}

fn profile_ref(name: &str, scope: &str) -> ProfileRef {
    ProfileRef {
        name: name.into(),
        scope: scope.into(),
    }
}

#[tokio::test]
async fn inspect_builds_the_facts_from_the_recorded_answers() {
    let server = MockServer::start().await;
    let ws = workspace_with(Some(OPENCODE_JSON), Some("decisions:\n  enabled: false\n"));
    let sys = systemctl(
        ws.path(),
        &format!("Environment=\"NEARAI_API_KEY=local\" \"XDG_CONFIG_HOME={TEST_XDG}\" "),
    );
    let runner = connected_at(&server, ws.path(), Some(&sys)).await;
    recorded(&fixture("playbook_detail_live"), false)
        .mount(&server)
        .await;
    recorded(&fixture("profile_detail_project_live"), false)
        .mount(&server)
        .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;

    let f = runner
        .inspect(&PlaybookId::new("repo-audit-fast"))
        .await
        .unwrap();
    assert_eq!(f.playbook_id, "repo-audit-fast");
    assert_eq!(
        f.default_profile,
        Some(profile_ref("nearbee-own", "auto")),
        "the default profile stays keyed by its reference"
    );
    assert_eq!(
        f.profiles,
        vec![(
            profile_ref("nearbee-own", "auto"),
            Known::Yes(nearbee_core::tiers::ProfileFact {
                reference: profile_ref("nearbee-own", "project"),
                executor: nearbee_core::tiers::ExecutorFact {
                    agent: "claude".into(),
                    model: "claude-haiku-5-5".into(),
                },
                fallbacks: vec![],
                tiers: vec![],
                environment: "minimal".into(),
                skills: vec![],
                env_policy: "allowlist".into(),
                env_allow: vec![],
            })
        )],
        "auto resolved the project scope only; the fact records the scope \
         it was read from, the outer key stays the reference"
    );
    let ids: Vec<&str> = f.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["done", "manifest", "start", "write"], "sorted by id");
    let write = f.nodes.iter().find(|n| n.id == "write").unwrap();
    assert_eq!(write.kind, "agent_task");
    assert!(!write.interactive);
    assert_eq!(write.profile, None);
    assert_eq!(write.connectors, Vec::<String>::new());
    assert_eq!(write.playbook_ref, None);
    let Known::Yes(routes) = &f.routes else {
        panic!("{f:?}")
    };
    assert_eq!(routes.len(), 2, "sorted by name");
    assert_eq!(routes[0].name, "nearai");
    assert_eq!(routes[0].base_url, NEARAI_BASE_URL);
    assert_eq!(routes[0].api_key_ref, "env:NEARAI_API_KEY");
    assert_eq!(routes[1].name, "openrouter");
    assert_eq!(f.decisions_enabled, Known::Yes(false));
    assert_eq!(f.compaction_enabled, Known::Yes(false));
}

#[tokio::test]
async fn a_scoped_reference_reads_only_its_scope_and_auto_falls_back() {
    let server = MockServer::start().await;
    let ws = workspace_with(None, None);
    let runner = connected_at(&server, ws.path(), None).await;
    // The mapping must sit under `profile:`, not beside it: sibling keys
    // at the node's indent leave `profile` null and no profile is fetched.
    let source = source_yaml(None, Some("    name: nearbee-near\n    scope: project"));
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "smoke-own",
            "yaml": source,
        })))
        .mount(&server)
        .await;
    recorded(&fixture("profile_detail_near_project_live"), false)
        .expect(1)
        .mount(&server)
        .await;
    // The project scope holds the profile: no global request at all.
    mock_profile(
        &server,
        "nearbee-near",
        "global",
        ResponseTemplate::new(500),
    )
    .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;

    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    assert_eq!(f.profiles.len(), 1);
    let (r, fact) = &f.profiles[0];
    assert_eq!(r, &profile_ref("nearbee-near", "project"));
    let Known::Yes(fact) = fact else {
        panic!("{fact:?}")
    };
    assert_eq!(fact.executor.model, "nearai/Qwen/Qwen3.8-27B");
    assert_eq!(
        fact.fallbacks,
        vec![nearbee_core::tiers::ExecutorFact {
            agent: "opencode".into(),
            model: "nearai/deepseek-ai/DeepSeek-V4.1-Flash-1017".into(),
        }]
    );
    assert_eq!(fact.env_allow, vec!["NEARAI_API_KEY".to_string()]);
    let global_hits = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.as_str().contains("scope=global"))
        .count();
    assert_eq!(global_hits, 0, "a scoped reference reads only its scope");
}

#[tokio::test]
async fn auto_falls_back_to_the_global_scope() {
    let server = MockServer::start().await;
    let ws = workspace_with(None, None);
    let runner = connected_at(&server, ws.path(), None).await;
    let source = source_yaml(Some("nearbee-own"), None);
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    mock_profile(
        &server,
        "nearbee-own",
        "project",
        ResponseTemplate::new(404),
    )
    .await;
    mock_profile(
        &server,
        "nearbee-own",
        "global",
        profile_answer(
            "nearbee-own",
            "global",
            "name: nearbee-own\nexecutor:\n  agent: claude\n  model: claude-haiku-5-5\n",
            200,
        ),
    )
    .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    let (r, fact) = &f.profiles[0];
    assert_eq!(r, &profile_ref("nearbee-own", "auto"));
    assert!(matches!(fact, Known::Yes(_)), "{fact:?}");
}

#[tokio::test]
async fn a_404_profile_is_unknown_with_the_status() {
    let server = MockServer::start().await;
    let ws = workspace_with(None, None);
    let runner = connected_at(&server, ws.path(), None).await;
    let source = source_yaml(Some("nearbee-own"), None);
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    mock_profile(
        &server,
        "nearbee-own",
        "project",
        ResponseTemplate::new(404),
    )
    .await;
    mock_profile(&server, "nearbee-own", "global", ResponseTemplate::new(404)).await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    let Known::Unknown(reason) = &f.profiles[0].1 else {
        panic!("{:?}'", f.profiles[0].1)
    };
    assert!(reason.contains("404"), "{reason}");
}

#[tokio::test]
async fn a_missing_opencode_json_makes_the_routes_unknown() {
    let server = MockServer::start().await;
    let ws = workspace_with(None, None);
    let runner = connected_at(&server, ws.path(), None).await;
    let source = source_yaml(None, None);
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    let Known::Unknown(reason) = &f.routes else {
        panic!("{:?}", f.routes)
    };
    assert!(reason.contains("opencode.json"), "{reason}");
}

#[tokio::test]
async fn an_opencode_variable_in_the_instance_environment_makes_the_routes_unknown() {
    let server = MockServer::start().await;
    let ws = workspace_with(Some(OPENCODE_JSON), None);
    let sys = systemctl(ws.path(), "Environment=\"OPENCODE_API_KEY=sk\" \"HOME=/h\"");
    let runner = connected_at(&server, ws.path(), Some(&sys)).await;
    let source = source_yaml(None, None);
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    let Known::Unknown(reason) = &f.routes else {
        panic!("{:?}", f.routes)
    };
    assert!(reason.contains("OPENCODE_"), "{reason}");
}

/// `NEARAI_CLOUD_URL`, a `NEARBEE_CLOUD_` variable or a proxy variable in
/// any case makes the routes unknown (task 3.1, spec `privacy-tiers`,
/// Direct call route).
#[tokio::test]
async fn a_redirecting_variable_in_the_instance_environment_makes_the_routes_unknown() {
    for var in [
        "NEARAI_CLOUD_URL=http://x",
        "NEARBEE_CLOUD_BASE=http://x",
        "https_proxy=http://p:3128",
        "HTTP_PROXY=http://p:3128",
        "All_Proxy=socks5://p",
    ] {
        let server = MockServer::start().await;
        let ws = workspace_with(Some(OPENCODE_JSON), None);
        let sys = systemctl(ws.path(), &format!("Environment=\"{var}\" \"HOME=/h\""));
        let runner = connected_at(&server, ws.path(), Some(&sys)).await;
        let source = source_yaml(None, None);
        Mock::given(method("GET"))
            .and(path("/api/playbooks/smoke-own"))
            .and(wiremock::matchers::query_param("workspace", WS))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id": "smoke-own", "yaml": source})),
            )
            .mount(&server)
            .await;
        recorded(&fixture("health_live"), false)
            .mount(&server)
            .await;
        let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
        let Known::Unknown(reason) = &f.routes else {
            panic!("{var}: {:?}", f.routes)
        };
        let name = var.split('=').next().unwrap();
        assert!(reason.contains(name), "{reason}");
    }
}

/// Mounts a playbook detail answer for `smoke-own` with `source` and,
/// unless `health` is false, the recorded health answer.
async fn mount_source(server: &MockServer, source: &str, health: bool) {
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(server)
        .await;
    if health {
        recorded(&fixture("health_live"), false).mount(server).await;
    }
}

/// A failed read of the instance environment says nothing about the
/// configuration: the whole inspection fails (spec `apb-runner`,
/// Playbook inspection: a failed read is an error, not an unknown fact).
#[tokio::test]
async fn a_failed_environment_read_fails_the_inspection() {
    let server = MockServer::start().await;
    let ws = workspace_with(Some(OPENCODE_JSON), None);
    let sys = systemctl(ws.path(), "");
    let runner = connected_at(&server, ws.path(), Some(&sys)).await;
    mount_source(&server, &source_yaml(None, None), true).await;
    let err = runner
        .inspect(&PlaybookId::new("smoke-own"))
        .await
        .expect_err("a failed environment read fails the inspection");
    assert!(err.to_string().contains("environment"), "{err}");
}

/// A profile read that fails on transport or a 5xx fails the inspection
/// instead of becoming an unknown profile (a transient error must not
/// move the fingerprint).
#[tokio::test]
async fn a_profile_read_error_fails_the_inspection() {
    let server = MockServer::start().await;
    let ws = workspace_with(Some(OPENCODE_JSON), Some("decisions:\n  enabled: false\n"));
    let runner = connected_at(&server, ws.path(), None).await;
    mount_source(&server, &source_yaml(Some("nearbee-own"), None), true).await;
    mock_profile(
        &server,
        "nearbee-own",
        "project",
        ResponseTemplate::new(503),
    )
    .await;
    runner
        .inspect(&PlaybookId::new("smoke-own"))
        .await
        .expect_err("a 503 on the profile fails the inspection");
}

/// A failed health read fails the inspection (the compaction fact).
#[tokio::test]
async fn a_health_read_error_fails_the_inspection() {
    let server = MockServer::start().await;
    let ws = workspace_with(Some(OPENCODE_JSON), Some("decisions:\n  enabled: false\n"));
    let runner = connected_at(&server, ws.path(), None).await;
    mount_source(&server, &source_yaml(None, None), false).await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    runner
        .inspect(&PlaybookId::new("smoke-own"))
        .await
        .expect_err("a failed health read fails the inspection");
}

/// The inspection's routes for a workspace, a fake `systemctl` line and
/// an expected `XDG_CONFIG_HOME`.
async fn routes_for(
    ws: &Path,
    env_line: Option<&str>,
    xdg: &Path,
) -> Known<Vec<nearbee_core::tiers::ProviderRoute>> {
    let server = MockServer::start().await;
    let sys = env_line.map(|line| systemctl(ws, line));
    let runner = connected_xdg(&server, ws, sys.as_deref(), xdg).await;
    mount_source(&server, &source_yaml(None, None), true).await;
    runner
        .inspect(&PlaybookId::new("smoke-own"))
        .await
        .unwrap()
        .routes
}

fn unknown_reason(routes: &Known<Vec<nearbee_core::tiers::ProviderRoute>>) -> &str {
    match routes {
        Known::Unknown(reason) => reason,
        Known::Yes(r) => panic!("the routes are known: {r:?}"),
    }
}

/// OpenCode's global config (spec `privacy-tiers`, OpenCode
/// configuration): the routes are unknown unless the instance's
/// `XDG_CONFIG_HOME` is Nearbee's own directory and that directory holds
/// no `opencode` entry.
#[tokio::test]
async fn the_routes_are_unknown_unless_xdg_config_home_is_nearbees_empty_directory() {
    let ws = workspace_with(Some(OPENCODE_JSON), None);
    let xdg = Path::new(TEST_XDG);
    // Not set at all.
    let r = routes_for(ws.path(), Some("Environment=HOME=/h"), xdg).await;
    assert!(
        unknown_reason(&r).contains("XDG_CONFIG_HOME is not set"),
        "{r:?}"
    );
    // Set to another directory (the owner's own config).
    let r = routes_for(
        ws.path(),
        Some("Environment=XDG_CONFIG_HOME=/home/o/.config"),
        xdg,
    )
    .await;
    assert!(unknown_reason(&r).contains("/home/o/.config"), "{r:?}");
    // Nearbee's directory, quoted the way systemd prints a value with a space.
    let spaced = tempfile::tempdir().unwrap();
    let dir = spaced.path().join("xdg config");
    std::fs::create_dir_all(&dir).unwrap();
    let r = routes_for(
        ws.path(),
        Some(&format!(
            "Environment=\"XDG_CONFIG_HOME={}\" HOME=/h",
            dir.display()
        )),
        &dir,
    )
    .await;
    assert!(matches!(r, Known::Yes(_)), "{r:?}");
    // The directory holds an OpenCode configuration.
    std::fs::create_dir_all(dir.join("opencode")).unwrap();
    std::fs::write(dir.join("opencode/opencode.json"), r#"{"mcp": {}}"#).unwrap();
    let r = routes_for(
        ws.path(),
        Some(&format!(
            "Environment=\"XDG_CONFIG_HOME={}\"",
            dir.display()
        )),
        &dir,
    )
    .await;
    assert!(
        unknown_reason(&r).contains("opencode configuration"),
        "{r:?}"
    );
}

/// A `.opencode/` directory in the workspace would be merged by OpenCode
/// (plugins, agents, MCP servers): the routes are unknown.
#[tokio::test]
async fn a_workspace_opencode_directory_makes_the_routes_unknown() {
    let ws = workspace_with(Some(OPENCODE_JSON), None);
    std::fs::create_dir_all(ws.path().join(".opencode/plugins")).unwrap();
    let r = routes_for(ws.path(), None, Path::new(TEST_XDG)).await;
    assert!(unknown_reason(&r).contains(".opencode"), "{r:?}");
}

/// OpenCode walks up from the working directory: an `opencode.json`
/// above the workspace, or a `.opencode/` there holding more than
/// OpenCode's own install files, makes the routes unknown.
#[tokio::test]
async fn an_opencode_config_above_the_workspace_makes_the_routes_unknown() {
    let outer = tempfile::tempdir().unwrap();
    let ws = outer.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("opencode.json"), OPENCODE_JSON).unwrap();
    // The installer's files alone are fine.
    std::fs::create_dir_all(outer.path().join(".opencode/bin")).unwrap();
    std::fs::write(outer.path().join(".opencode/package.json"), "{}").unwrap();
    let r = routes_for(&ws, None, Path::new(TEST_XDG)).await;
    assert!(matches!(r, Known::Yes(_)), "{r:?}");
    // An agent definition there is configuration.
    std::fs::create_dir_all(outer.path().join(".opencode/agent")).unwrap();
    let r = routes_for(&ws, None, Path::new(TEST_XDG)).await;
    assert!(unknown_reason(&r).contains("holds agent"), "{r:?}");
    std::fs::remove_dir_all(outer.path().join(".opencode/agent")).unwrap();
    // A config file there.
    std::fs::write(outer.path().join("opencode.jsonc"), "{}").unwrap();
    let r = routes_for(&ws, None, Path::new(TEST_XDG)).await;
    assert!(unknown_reason(&r).contains("opencode.jsonc"), "{r:?}");
}

/// Model aliases and overrides (spec `privacy-tiers`, Attested model
/// route): a model entry with any key beyond display and metadata is
/// recorded on the route; metadata alone is not.
#[tokio::test]
async fn model_aliases_and_overrides_are_read_into_the_route() {
    let ws = workspace_with(
        Some(
            r#"{"provider": {"nearai": {"options": {"baseURL": "https://cloud-api.near.ai/v1", "apiKey": "{env:NEARAI_API_KEY}"},
              "models": {
                "Qwen/Qwen3.8-27B": {"name": "Qwen", "id": "anthropic/claude-fable-5"},
                "deepseek-ai/DeepSeek-V4.1-Flash-1017": {"name": "DeepSeek", "limit": {"context": 1}},
                "zai/GLM": {"options": {"baseURL": "http://evil"}}
              }}}}"#,
        ),
        None,
    );
    let r = routes_for(ws.path(), None, Path::new(TEST_XDG)).await;
    let Known::Yes(routes) = r else {
        panic!("{r:?}")
    };
    assert_eq!(
        routes[0].model_overrides,
        vec![
            "Qwen/Qwen3.8-27B: id".to_string(),
            "zai/GLM: options".to_string()
        ]
    );
    assert_eq!(routes[0].api_key_ref, "{env:NEARAI_API_KEY}");
}

/// A malformed fallback or tier entry makes the whole profile unknown;
/// it is never dropped in silence (an entry the facts cannot read may
/// route to a model the check never saw).
#[tokio::test]
async fn a_malformed_fallback_or_tier_makes_the_profile_unknown() {
    let cases = [
        "name: p\nexecutor:\n  agent: opencode\n  model: nearai/Qwen/Qwen3.8-27B\n  fallbacks:\n    - agent: claude\n",
        "name: p\nexecutor:\n  agent: opencode\n  model: nearai/Qwen/Qwen3.8-27B\n  fallbacks: claude\n",
        "name: p\nexecutor:\n  agent: opencode\n  model: nearai/Qwen/Qwen3.8-27B\ntiers:\n  long:\n    agent: claude\n",
        "name: p\nexecutor:\n  agent: opencode\n  model: nearai/Qwen/Qwen3.8-27B\ntiers: [a]\n",
    ];
    for yaml in cases {
        let server = MockServer::start().await;
        let ws = workspace_with(None, None);
        let runner = connected_at(&server, ws.path(), None).await;
        mount_source(&server, &source_yaml(Some("p"), None), true).await;
        mock_profile(
            &server,
            "p",
            "project",
            profile_answer("p", "project", yaml, 200),
        )
        .await;
        let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
        let Known::Unknown(reason) = &f.profiles[0].1 else {
            panic!("{yaml}: {:?}", f.profiles[0].1)
        };
        assert!(reason.contains("does not parse"), "{yaml}: {reason}");
    }
}

#[tokio::test]
async fn decisions_follow_the_project_config() {
    let server = MockServer::start().await;
    let ws = workspace_with(None, Some("decisions:\n  enabled: true\n"));
    let runner = connected_at(&server, ws.path(), None).await;
    let source = source_yaml(None, None);
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    assert_eq!(f.decisions_enabled, Known::Yes(true));

    // Absent: unknown, never guessed.
    let server = MockServer::start().await;
    let ws = workspace_with(None, Some("decisions:\n  enabled: null\n"));
    let runner = connected_at(&server, ws.path(), None).await;
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    assert!(
        matches!(&f.decisions_enabled, Known::Unknown(reason) if reason.contains("not set")),
        "{:?}",
        f.decisions_enabled
    );
}

#[tokio::test]
async fn compaction_follows_the_health_version() {
    let server = MockServer::start().await;
    let ws = workspace_with(None, None);
    let runner = connected_at(&server, ws.path(), None).await;
    let source = source_yaml(None, None);
    Mock::given(method("GET"))
        .and(path("/api/playbooks/smoke-own"))
        .and(wiremock::matchers::query_param("workspace", WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "smoke-own", "yaml": source})),
        )
        .mount(&server)
        .await;
    let mut health = fixture("health_live");
    health["response"]["body"]["version"] = json!("1.0.0-beta.5");
    recorded(&health, false).mount(&server).await;
    let f = runner.inspect(&PlaybookId::new("smoke-own")).await.unwrap();
    let Known::Unknown(reason) = &f.compaction_enabled else {
        panic!("{:?}", f.compaction_enabled)
    };
    assert!(reason.contains("apb version 1.0.0-beta.5"), "{reason}");
}

#[tokio::test]
async fn inspect_source_gives_the_same_facts_as_inspect() {
    let server = MockServer::start().await;
    let ws = workspace_with(Some(OPENCODE_JSON), Some("decisions:\n  enabled: false\n"));
    let runner = connected_at(&server, ws.path(), None).await;
    let f = fixture("playbook_detail_live");
    recorded(&f, false).mount(&server).await;
    recorded(&fixture("profile_detail_project_live"), false)
        .mount(&server)
        .await;
    recorded(&fixture("health_live"), false)
        .mount(&server)
        .await;
    let from_route = runner
        .inspect(&PlaybookId::new("repo-audit-fast"))
        .await
        .unwrap();
    let from_source = runner
        .inspect_source(f["response"]["body"]["yaml"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(from_route, from_source);
}

/// Spec `privacy-tiers`, Sandbox fact: the inspection reads the unit's
/// ExecStart and reports the sandbox with its hosts; a unit that is not
/// the sandbox, or an ExecStart read that fails while the environment
/// read works, gives no fact and no failed inspection.
#[tokio::test]
async fn inspect_reports_the_sandbox_from_the_units_exec_start() {
    let exec_start = "ExecStart={ path=/usr/bin/docker ; argv[]=/usr/bin/docker run --rm --network nearbee-sbx-int --dns 127.0.0.1 --add-host github.com:172.30.77.2 --add-host cloud-api.near.ai:172.30.77.2 --read-only --cap-drop ALL --security-opt no-new-privileges nearbee-sandbox:local apb dashboard --port 7322 ; ignore_errors=no ; status=0/0 }";
    let cases = [
        (
            exec_start.to_owned(),
            Some(vec![
                "cloud-api.near.ai".to_owned(),
                "github.com".to_owned(),
            ]),
        ),
        (exec_start.replace(" --read-only", ""), None),
        (String::new(), None),
    ];
    for (line, expected) in cases {
        let server = MockServer::start().await;
        let ws = workspace_with(Some(OPENCODE_JSON), Some("decisions:\n  enabled: false\n"));
        let sys = ws.path().join("systemctl");
        let exec_branch = if line.is_empty() {
            "exit 1".to_owned()
        } else {
            format!("printf '%s\\n' '{line}'")
        };
        std::fs::write(
            &sys,
            format!(
                "#!/bin/sh\nif [ \"$5\" = ExecStart ]; then {exec_branch}; else printf '%s\\n' 'Environment=XDG_CONFIG_HOME={TEST_XDG}'; fi\n"
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sys, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = connected_at(&server, ws.path(), Some(&sys)).await;
        recorded(&fixture("playbook_detail_live"), false)
            .mount(&server)
            .await;
        recorded(&fixture("profile_detail_project_live"), false)
            .mount(&server)
            .await;
        recorded(&fixture("health_live"), false)
            .mount(&server)
            .await;
        let f = runner
            .inspect(&PlaybookId::new("repo-audit-fast"))
            .await
            .unwrap();
        assert_eq!(
            f.sandbox.map(|s| s.allowed_hosts),
            expected,
            "ExecStart line: {line:?}"
        );
        assert!(matches!(f.routes, Known::Yes(_)), "{:?}", f.routes);
    }
}

// --- Attempts (spec `apb-runner`, Requested models per attempt) ---

#[tokio::test]
async fn attempts_are_read_from_the_run_answer() {
    let f = fixture("run_succeeded");
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&f, true).mount(&server).await;
    let d = runner
        .run(&RunId::new("smoke-own-1794253399097"))
        .await
        .unwrap();
    assert_eq!(
        d.attempts,
        vec![AttemptModel {
            node: "write".into(),
            attempt: 1,
            agent: "claude".into(),
            requested_model: Some("claude-haiku-5-5".into()),
            reported: Reported::None,
        }],
        "no reported model stays empty, never filled from the requested one"
    );

    // One reported model.
    let f = fixture("run_attempt_one");
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&f, true).mount(&server).await;
    let d = runner.run(&RunId::new("run-attempt-one")).await.unwrap();
    assert_eq!(
        d.attempts[0].reported,
        Reported::One("nearai/Qwen/Qwen3.8-27B".into())
    );

    // Mixed, next to an attempt with none.
    let f = fixture("run_attempt_mixed");
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&f, true).mount(&server).await;
    let d = runner.run(&RunId::new("run-attempt-mixed")).await.unwrap();
    assert_eq!(d.attempts[0].reported, Reported::Mixed);
    assert_eq!(d.attempts[0].node, "write");
    assert_eq!(d.attempts[1].reported, Reported::None);
    assert_eq!(d.attempts[1].agent, "sh");
    assert_eq!(d.attempts[1].requested_model, None);

    // A run answer without attempt_models: empty, not an error.
    let mut body = fixture("run_succeeded")["response"]["body"].clone();
    body.as_object_mut().unwrap().remove("attempt_models");
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded_with(
        &fixture("run_succeeded"),
        true,
        ResponseTemplate::new(200).set_body_json(body),
    )
    .mount(&server)
    .await;
    let d = runner
        .run(&RunId::new("smoke-own-1794253399097"))
        .await
        .unwrap();
    assert!(d.attempts.is_empty());
}

// --- External-data runs and the instance version (S6 design 3) ---

/// A health answer with the given version, on the recorded beta.7 route.
async fn supported_with(version: Value) -> Result<bool, RunnerError> {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("health_beta7_live");
    let mut body = f["response"]["body"].clone();
    body["version"] = version;
    recorded_with(&f, false, ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    runner.external_data_supported().await
}

#[tokio::test]
async fn external_data_support_follows_the_prerelease_order() {
    // The recorded beta.7 answer as it is.
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&fixture("health_beta7_live"), false)
        .expect(1)
        .mount(&server)
        .await;
    assert!(runner.external_data_supported().await.unwrap());
    assert!(!supported_with(json!("1.0.0-beta.6")).await.unwrap());
    assert!(supported_with(json!("1.0.0-beta.10")).await.unwrap());
    assert!(supported_with(json!("1.0.0")).await.unwrap());
}

#[tokio::test]
async fn an_unparsable_version_is_a_protocol_error_naming_the_field() {
    for v in [json!("dev"), Value::Null] {
        let err = supported_with(v).await.unwrap_err();
        let RunnerError::Protocol { detail } = &err else {
            panic!("{err:?}")
        };
        assert!(detail.contains("version"), "{detail}");
    }
}

#[test]
fn compaction_is_known_off_only_for_the_read_releases() {
    use nearbee_apb::compaction_for_version;
    assert_eq!(
        compaction_for_version(Some("1.0.0-beta.6")),
        Known::Yes(false)
    );
    assert_eq!(
        compaction_for_version(Some("1.0.0-beta.7")),
        Known::Yes(false)
    );
    assert!(matches!(
        compaction_for_version(Some("1.0.0-beta.8")),
        Known::Unknown(_)
    ));
    assert!(matches!(compaction_for_version(None), Known::Unknown(_)));
}

/// The comparison is by parsed version: build metadata is ignored, and a
/// string that is not a version stays unknown.
#[test]
fn compaction_ignores_build_metadata() {
    use nearbee_apb::compaction_for_version;
    assert_eq!(
        compaction_for_version(Some("1.0.0-beta.7+abc")),
        Known::Yes(false)
    );
    assert!(matches!(
        compaction_for_version(Some("1.0.0-beta.8+abc")),
        Known::Unknown(_)
    ));
    assert!(matches!(
        compaction_for_version(Some("not a version")),
        Known::Unknown(_)
    ));
}

#[tokio::test]
async fn the_start_body_carries_the_flag_only_when_set() {
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    let f = fixture("start");
    let (mut req, mut body, _dir) = start_request(&f, "smoke-own");
    req.external_data = true;
    body["external_data"] = json!(true);
    builder(&f, false)
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": "flagged"})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(runner.start(req).await.unwrap().as_str(), "flagged");
    // Without the flag the body has no `external_data` key at all (the
    // recorded body has none).
    let (req2, body2, _dir2) = start_request(&f, "smoke-own");
    assert!(!req2.external_data);
    assert!(body2.get("external_data").is_none());
    builder(&f, false)
        .and(body_json(body2))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": "plain"})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(runner.start(req2).await.unwrap().as_str(), "plain");
}

#[tokio::test]
async fn the_detail_reports_the_flag_only_when_it_carries_it() {
    // The recorded flagged run of a throwaway script-only playbook.
    let server = MockServer::start().await;
    let runner = connected(&server).await;
    recorded(&fixture("run_external_data_live"), false)
        .mount(&server)
        .await;
    let d = runner
        .run(&RunId::new("s6-flag-probe-1794411151095"))
        .await
        .unwrap();
    assert!(d.external_data);
    assert_eq!(d.status, RunStatus::Succeeded);
    // A beta.6 detail has no field: not an external-data run.
    let body = fixture("run_succeeded")["response"]["body"].clone();
    assert!(body.get("external_data").is_none());
    assert!(!run_with(body.clone()).await.external_data);
    let mut f = body;
    f["external_data"] = json!(false);
    assert!(!run_with(f).await.external_data);
}
