//! Contract tests of `NearAgentMarket` against wiremock: transport rules,
//! retry classes, paging, write resend rules, the file upload trio and the
//! mapping of every parsed answer. Every mocked body is schema-shaped: it
//! comes from the shared fixture builder (the dev-only crate
//! `nearbee-market-fixture`), which builds it from the vendored OpenAPI,
//! never from what the client reads.

use std::time::{Duration, Instant};

use nearbee_core::market::{
    AgentPatch, DeclineOutcome, DeclineReason, DeclineRequest, Market, MarketError, MessageRequest,
    NewPricingPlan, ProgressRequest, RowKind, RowStatus, SubmitRequest, UploadFile,
};
use nearbee_core::secret::Secret;
use nearbee_market::{MarketOptions, MarketTiming, NearAgentMarket};
use nearbee_market_fixture::{self as fx, instance, instance_with};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, body_string, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn timing(write_timeout: Duration) -> MarketTiming {
    MarketTiming {
        connect: Duration::from_secs(2),
        read: Duration::from_secs(5),
        write: write_timeout,
        read_retry_delays: vec![
            Duration::from_millis(10),
            Duration::from_millis(10),
            Duration::from_millis(10),
        ],
        write_resend_delay: Duration::from_millis(20),
        retry_after_ceiling: Duration::from_secs(10),
    }
}

async fn client(server: &MockServer, write_timeout: Duration) -> NearAgentMarket {
    NearAgentMarket::new(MarketOptions {
        base_url: server.uri(),
        token: Secret::new("aat_0123456789abcdef0123456789abcdef"),
        redact: Vec::new(),
        timing: timing(write_timeout),
    })
    .unwrap()
}

fn ok_json(v: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(v)
}

/// A schema-shaped assignment row with only id and status chosen.
fn row(id: &str, status: &str) -> Value {
    fx::assignment_item(id, status, Value::Null, Value::Null)
}

fn invalid_state(message: &str) -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_json(fx::refusal("invalid_state_transition", message))
}

const TOKEN: &str = "aat_0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn a_3xx_on_a_write_is_unknown_and_not_followed() {
    let server = MockServer::start().await;
    // The Location points at this server too, so a followed redirect would
    // be visible as a request to /elsewhere.
    Mock::given(method("POST"))
        .and(path("/elsewhere"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/submit"))
        .and(header("Authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/elsewhere", server.uri())),
        )
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let err = client
        .submit(
            "a1",
            &SubmitRequest {
                markdown: "# result".into(),
                files: vec![],
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, MarketError::WriteOutcomeUnknown { .. }),
        "{err}"
    );
    // The redirect was not followed and the one resend saw the same 3xx:
    // exactly two requests, never a request to the Location.
}

#[tokio::test]
async fn a_401_is_unauthorized_and_never_retried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(fx::refusal("unauthorized", "bad token")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let err = client.assignments().await.unwrap_err();
    assert!(matches!(err, MarketError::Unauthorized { .. }), "{err}");
    assert!(err.to_string().contains("unauthorized"));
}

#[tokio::test]
async fn capability_links_and_tokens_are_redacted_in_errors() {
    let server = MockServer::start().await;
    let leak = format!("see /files/deadbeef00/x and {TOKEN} at https://m.near.ai/up?sig=abc");
    Mock::given(method("GET"))
        .and(path("/v1/agents/a1"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(fx::refusal("validation_error", &leak)),
        )
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let err = client.agent("a1").await.unwrap_err();
    let detail = err.to_string();
    assert!(!detail.contains("deadbeef00"), "{detail}");
    assert!(!detail.contains(TOKEN), "{detail}");
    assert!(!detail.contains("sig=abc"), "{detail}");
    assert!(detail.contains("/files/<redacted>"), "{detail}");
}

#[tokio::test]
async fn a_read_is_retried_after_503_honouring_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "2"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![])))
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let started = Instant::now();
    let rows = client.assignments().await.unwrap();
    assert!(rows.is_empty());
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "retry ignored Retry-After: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn paging_reads_two_pages_and_stops() {
    let server = MockServer::start().await;
    let page: Vec<Value> = (0..100)
        .map(|i| row(&format!("a{i}"), "in_progress"))
        .collect();
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .and(query_param("status", "all"))
        .and(query_param("limit", "100"))
        .and(query_param("offset", "0"))
        .respond_with(ok_json(fx::assignments(page)))
        .expect(1)
        .mount(&server)
        .await;
    let tail: Vec<Value> = (0..3).map(|i| row(&format!("b{i}"), "cancelled")).collect();
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .and(query_param("offset", "100"))
        .respond_with(ok_json(fx::assignments(tail)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let rows = client.assignments().await.unwrap();
    assert_eq!(rows.len(), 103);
    assert_eq!(rows[0].id, "a0");
    assert_eq!(rows[102].id, "b2");
    assert_eq!(rows[102].status(), RowStatus::Cancelled);
}

#[tokio::test]
async fn an_empty_list_is_one_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![])))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    assert!(client.assignments().await.unwrap().is_empty());
}

/// A write whose first attempt times out is resent with an identical body.
#[tokio::test]
async fn a_timed_out_submit_is_resent_identically() {
    let server = MockServer::start().await;
    let body = json!({"deliverableMarkdown": "# result", "files": ["u1"]});
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/submit"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/submit"))
        .and(body_string(body.to_string()))
        .respond_with(ok_json(fx::write_answer("JobAssignment")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_millis(300)).await;
    client
        .submit(
            "a1",
            &SubmitRequest {
                markdown: "# result".into(),
                files: vec!["u1".into()],
            },
        )
        .await
        .unwrap();
}

/// A write whose outcome stays unknown after the one resend is reported as
/// unknown after exactly two requests.
#[tokio::test]
async fn a_write_is_reported_unknown_after_exactly_two_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/start"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_millis(300)).await;
    let err = client.start("a1").await.unwrap_err();
    assert!(
        matches!(err, MarketError::WriteOutcomeUnknown { .. }),
        "{err}"
    );
}

/// A decline repeat answered `invalid_state_transition` counts as done only
/// when a fresh read shows the row cancelled.
#[tokio::test]
async fn a_decline_repeat_with_a_cancelled_row_is_done() {
    let server = MockServer::start().await;
    let refused = invalid_state("cannot transition from 'cancelled' to 'cancelled'");
    // First attempt 5xx (unknown), repeat invalid_state_transition.
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(refused)
        .mount(&server)
        .await;
    // The fresh read goes through the paged status=all list: the worker
    // API has no single-row assignment route.
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![row("a1", "cancelled")])))
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let out = client
        .decline(
            "a1",
            &DeclineRequest {
                reason: DeclineReason::Failed,
                detail: Some("run failed".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(out, DeclineOutcome::Done);
}

#[tokio::test]
async fn a_decline_repeat_with_a_live_row_is_refused() {
    let server = MockServer::start().await;
    let refused = invalid_state("cannot transition from 'accepted' to 'cancelled'");
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(refused)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![row("a1", "accepted")])))
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let out = client
        .decline(
            "a1",
            &DeclineRequest {
                reason: DeclineReason::Rejected,
                detail: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(out, DeclineOutcome::Refused);
}

/// The decline repeat happens at most once: after the repeat answers
/// `invalid_state_transition` no third request is made.
#[tokio::test]
async fn a_decline_repeat_happens_at_most_once() {
    let server = MockServer::start().await;
    // First attempt unknown, the repeat answers invalid_state_transition,
    // and the fresh read settles it as done.
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(invalid_state("cannot transition"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![row("a1", "cancelled")])))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let out = client
        .decline(
            "a1",
            &DeclineRequest {
                reason: DeclineReason::Failed,
                detail: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(out, DeclineOutcome::Done);
}

/// A still-unknown decline stops after exactly two requests.
#[tokio::test]
async fn a_still_unknown_decline_stops_after_two_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/decline"))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let err = client
        .decline(
            "a1",
            &DeclineRequest {
                reason: DeclineReason::Failed,
                detail: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, MarketError::WriteOutcomeUnknown { .. }),
        "{err}"
    );
}

/// A success on a write whose body is not JSON leaves the outcome unknown,
/// after exactly two requests.
#[tokio::test]
async fn a_malformed_success_on_a_write_is_unknown() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/start"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .expect(2)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let err = client.start("a1").await.unwrap_err();
    assert!(
        matches!(err, MarketError::WriteOutcomeUnknown { .. }),
        "{err}"
    );
}

/// A message resend keeps the same `clientRequestId`.
#[tokio::test]
async fn a_message_resend_keeps_the_same_client_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/messages"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/messages"))
        .and(body_partial_json(
            json!({"clientRequestId": "cr-1", "body": "hi"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(fx::write_answer("ThreadMessage")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    client
        .message(
            "a1",
            &MessageRequest {
                body: "hi".into(),
                client_request_id: "cr-1".into(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn progress_after_409_waits_the_given_delay_before_its_one_repeat() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/progress"))
        .respond_with(ResponseTemplate::new(409).set_body_json({
            let mut b = fx::refusal("progress_report_too_soon", "too soon");
            b["retry_after"] = json!(2);
            b
        }))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/progress"))
        .and(body_partial_json(json!({"message": "working on it"})))
        .respond_with(ok_json(fx::write_answer("WorkerProgressReport")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let started = Instant::now();
    client
        .progress(
            "a1",
            &ProgressRequest {
                message: "working on it".into(),
                percent: None,
            },
        )
        .await
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "progress repeat ignored retry_after: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn messages_carry_the_client_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/messages"))
        .and(body_partial_json(json!({
            "body": "started work",
            "clientRequestId": "m-1"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(fx::write_answer("ThreadMessage")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    client
        .message(
            "a1",
            &MessageRequest {
                body: "started work".into(),
                client_request_id: "m-1".into(),
            },
        )
        .await
        .unwrap();
}

/// The reserve, PUT, complete trio: the bytes go to the presigned URL
/// exactly, with no extra headers, and the completion names the file.
#[tokio::test]
async fn the_file_upload_trio() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .and(body_partial_json(
            json!({"filename": "deliverable.md", "byteSize": 14, "link": true}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(fx::reserve_file_response(
                "f-1",
                &format!("{}/put-here", server.uri()),
                &format!("{}/files/f-1/deliverable.md", server.uri()),
            )),
        )
        .expect(1)
        .mount(&server)
        .await;
    let bytes = b"# deliverable\n".to_vec();
    struct NoExtraHeaders;
    impl wiremock::Match for NoExtraHeaders {
        fn matches(&self, req: &wiremock::Request) -> bool {
            req.headers.get("Authorization").is_none() && req.headers.get("Content-Type").is_none()
        }
    }
    let no_extra_headers = NoExtraHeaders;
    Mock::given(method("PUT"))
        .and(path("/put-here"))
        .and(body_string(String::from_utf8(bytes.clone()).unwrap()))
        // No extra headers: no bearer auth, no JSON content type.
        .and(no_extra_headers)
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/files/f-1/complete"))
        .and(NoBody)
        .respond_with(ok_json(fx::write_answer("CompleteFileResponse")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let up = client
        .upload_file(&UploadFile {
            name: "deliverable.md".into(),
            bytes,
        })
        .await
        .unwrap();
    assert_eq!(up.file_id, "f-1");
    assert_eq!(
        up.file_url,
        format!("{}/files/f-1/deliverable.md", server.uri())
    );
}

/// Matches a request that carries no body and no content type: the
/// operations `start`, `files/{id}/complete` and `make-default` define no
/// request body.
struct NoBody;
impl wiremock::Match for NoBody {
    fn matches(&self, req: &wiremock::Request) -> bool {
        req.body.is_empty() && req.headers.get("Content-Type").is_none()
    }
}

/// The assignments answer is the `MyAssignmentsResponse` envelope; each
/// `MyAssignmentItem` (nested `assignment`, `job`, `latestMessage`,
/// `autoAcceptAt`) maps onto Nearbee's row (codex 1).
#[tokio::test]
async fn an_assignment_item_maps_onto_the_row() {
    let server = MockServer::start().await;
    let assignment = instance_with(
        "JobAssignment",
        &["startedAt", "submittedAt", "deliverableUrl", "cancelReason"],
        json!({
            "assignmentId": "hire-1",
            "status": "in_progress",
            "slaDeadlineAt": "2027-11-20T12:00:00Z",
            "startedAt": "2026-11-09T10:00:00Z",
            "submittedAt": "2026-11-09T11:00:00Z",
            "deliverableUrl": "https://market.near.ai/files/t/old.md",
        }),
    );
    let job = instance(
        "WorkerJobBrief",
        json!({ "title": "Audit", "description": "Audit the repository." }),
    );
    let latest = instance(
        "LatestMessageBrief",
        json!({ "senderSide": "buyer", "body": "please also check CI" }),
    );
    let item = instance(
        "MyAssignmentItem",
        json!({
            "assignment": assignment,
            "job": job,
            "latestMessage": latest,
            "autoAcceptAt": "2026-11-16T11:00:00Z",
        }),
    );
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![item])))
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let rows = client.assignments().await.unwrap();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.id, "hire-1");
    assert_eq!(r.status(), RowStatus::InProgress);
    assert_eq!(r.kind(), RowKind::Rework);
    assert_eq!(r.title.as_deref(), Some("Audit"));
    assert_eq!(r.description.as_deref(), Some("Audit the repository."));
    assert_eq!(
        r.sla_deadline_at.unwrap().unix_timestamp(),
        nearbee_market::wire::wire_time("2027-11-20T12:00:00Z")
            .unwrap()
            .unix_timestamp()
    );
    assert!(r.started_at.is_some());
    assert!(r.auto_accept_at.is_some());
    let m = r.latest_message.as_ref().unwrap();
    assert_eq!(m.body, "please also check CI");
    assert_eq!(m.sender_side.as_deref(), Some("buyer"));

    // A minimal row (no optional fields) is first-time work with no
    // message.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![row("a1", "in_progress")])))
        .mount(&server)
        .await;
    let rows = client_for(&server).await.assignments().await.unwrap();
    assert_eq!(rows[0].kind(), RowKind::FirstTime);
    assert!(rows[0].latest_message.is_none());
}

async fn client_for(server: &MockServer) -> NearAgentMarket {
    client(server, Duration::from_secs(5)).await
}

/// A flat array (the shape the old mocks used) is not the API's answer and
/// is refused as a protocol error, never read as an empty queue.
#[tokio::test]
async fn a_flat_row_array_is_a_protocol_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(json!([{"id": "a1", "status": "in_progress"}])))
        .mount(&server)
        .await;
    let err = client_for(&server).await.assignments().await.unwrap_err();
    assert!(matches!(err, MarketError::Protocol { .. }), "{err}");
}

/// The public read answers either agent view; both parse with the
/// snake_case `agent_id` and `listing_status` (codex 2).
#[tokio::test]
async fn the_agent_read_parses_the_public_and_the_owner_view() {
    for (body, label) in [
        (fx::agent_public("ag-1", "repo-audit", "draft"), "public"),
        (fx::owner_agent_view("ag-1", "repo-audit", "live"), "owner"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/agents/repo-audit"))
            .respond_with(ok_json(body))
            .mount(&server)
            .await;
        let a = client_for(&server).await.agent("repo-audit").await.unwrap();
        assert_eq!(a.agent_id, "ag-1", "{label}");
        assert_eq!(a.handle.as_deref(), Some("repo-audit"), "{label}");
        assert!(a.listing_status.is_some(), "{label}");
    }
}

/// The PATCH sends snake_case fields of `PatchAgentRequest` (never
/// `listing_status`) and parses the `OwnerAgentView` answer (codex 2).
#[tokio::test]
async fn the_agent_patch_sends_snake_case_and_parses_the_owner_view() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/agents/ag-1"))
        .and(body_partial_json(json!({
            "bio_markdown": "# Repo audit",
            "category": "dev_tools",
            "sla_seconds": 3600
        })))
        .respond_with(ok_json(fx::owner_agent_view("ag-1", "repo-audit", "draft")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_for(&server).await;
    let a = client
        .patch_agent(
            "ag-1",
            &AgentPatch {
                name: Some("Repo audit".into()),
                bio_markdown: Some("# Repo audit".into()),
                category: Some("dev_tools".into()),
                sla_seconds: Some(3600),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(a.agent_id, "ag-1");
    assert_eq!(a.listing_status.as_deref(), Some("draft"));
    let sent = &server.received_requests().await.unwrap()[0];
    let body: Value = serde_json::from_slice(&sent.body).unwrap();
    assert!(body.get("listing_status").is_none(), "{body}");

    // A category outside the market's set is refused before any request.
    let err = client
        .patch_agent(
            "ag-1",
            &AgentPatch {
                category: Some("gardening".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, MarketError::Protocol { .. }), "{err}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// Plans are read and created in the `PricingPlan` names
/// (`pricing_plan_id`, `price_amount`, `is_default`), and make-default
/// sends no body (codex 3).
#[tokio::test]
async fn pricing_plans_list_create_and_make_default_use_the_component_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/ag-1/pricing-plans"))
        .respond_with(ok_json(json!([
            fx::pricing_plan("p-old", "ag-1", "1.00", Some(3600), true),
            fx::pricing_plan("p-other", "ag-1", "2.00", None, false),
        ])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/agents/ag-1/pricing-plans"))
        .and(body_partial_json(json!({
            "model": "per_call",
            "price_amount": "0.50",
            "sla_seconds": 86400
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(fx::pricing_plan(
            "p-new",
            "ag-1",
            "0.50",
            Some(86400),
            false,
        )))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pricing-plans/p-new/make-default"))
        .and(NoBody)
        .respond_with(ok_json(fx::pricing_plan(
            "p-new",
            "ag-1",
            "0.50",
            Some(86400),
            true,
        )))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_for(&server).await;
    let plans = client.pricing_plans("ag-1").await.unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].pricing_plan_id, "p-old");
    assert_eq!(plans[0].price_amount, "1.00");
    assert!(plans[0].is_default);
    assert_eq!(plans[0].model.as_deref(), Some("per_call"));
    assert_eq!(plans[0].sla_seconds, Some(3600));
    assert_eq!(plans[1].sla_seconds, None);
    let created = client
        .create_pricing_plan(
            "ag-1",
            &NewPricingPlan {
                price_usd: "0.50".into(),
                sla_seconds: 86400,
                display_name: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(created.pricing_plan_id, "p-new");
    client.make_default("ag-1", "p-new").await.unwrap();
}

/// `start` sends no body (the operation defines none) and accepts the
/// `StartAssignmentResponse` answer.
#[tokio::test]
async fn start_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/start"))
        .and(NoBody)
        .respond_with(ok_json(fx::write_answer("StartAssignmentResponse")))
        .expect(1)
        .mount(&server)
        .await;
    client_for(&server).await.start("a1").await.unwrap();
}

/// A linked reserve that answers without `fileUrl` cannot be delivered:
/// a protocol error before any bytes move.
#[tokio::test]
async fn a_linked_reserve_without_a_file_url_is_refused_before_the_put() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(instance("ReserveFileResponse", Value::Null)),
        )
        .mount(&server)
        .await;
    let err = client_for(&server)
        .await
        .upload_file(&UploadFile {
            name: "a.md".into(),
            bytes: b"a".to_vec(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, MarketError::Protocol { .. }), "{err}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
