//! Contract tests of `NearAgentMarket` against wiremock: transport rules,
//! retry classes, paging, write resend rules, the file upload trio and the
//! mapping of every parsed answer. Every mocked body is schema-shaped: it
//! comes from the shared fixture builder (the dev-only crate
//! `nearbee-market-fixture`), which builds it from the vendored OpenAPI,
//! never from what the client reads.

use std::time::{Duration, Instant};

use nearbee_core::market::{
    AgentPatch, DeclineOutcome, DeclineReason, DeclineRequest, Market, MarketError, MessageRequest,
    NewPricingPlan, ProgressOutcome, ProgressRequest, RowKind, RowStatus, StoredFile,
    SubmitRequest, UploadFile,
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
        read_budget: Duration::from_secs(2),
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
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "1"))
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
        started.elapsed() >= Duration::from_secs(1),
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

/// `409 progress_report_too_soon` returns its delay at once, without
/// waiting and without repeating (spec `market-client`, Decline and
/// progress repeats).
#[tokio::test]
async fn progress_returns_the_too_soon_delay_at_once_without_repeating() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/a1/progress"))
        .respond_with(ResponseTemplate::new(409).set_body_json({
            let mut b = fx::refusal("progress_report_too_soon", "too soon");
            b["retry_after"] = json!(12);
            b
        }))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Duration::from_secs(5)).await;
    let started = Instant::now();
    let out = client
        .progress(
            "a1",
            &ProgressRequest {
                message: "working on it".into(),
                percent: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        out,
        ProgressOutcome::TooSoon {
            retry_after_secs: 12
        },
        "the delay travels to the caller"
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the call must not wait: {:?}",
        started.elapsed()
    );
}

/// The read time budget: a `Retry-After` that would pass the 30 s total
/// ends the call at once as a retryable read (here proven with a 2 s
/// budget and a 120 s Retry-After).
#[tokio::test]
async fn a_retry_after_beyond_the_read_budget_ends_the_call_at_once() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "120"))
        .expect(1)
        .mount(&server)
        .await;
    let rows = client(&server, Duration::from_secs(5))
        .await
        .assignments()
        .await
        .unwrap_err();
    match rows {
        MarketError::RetryableRead {
            retry_after_secs, ..
        } => assert_eq!(retry_after_secs, Some(120)),
        other => panic!("{other:?}"),
    }
}

/// C15: the read budget bounds the whole call, request time included: a
/// retry after a `Retry-After` inside the budget gets only the time left
/// (2 s budget, 1 s wait, an answer that would take 4 s).
#[tokio::test]
async fn the_read_budget_caps_every_attempt_to_the_time_left() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(
            ok_json(fx::assignments(vec![row("a1", "in_progress")]))
                .set_delay(Duration::from_secs(4)),
        )
        .mount(&server)
        .await;
    let started = std::time::Instant::now();
    let err = client(&server, Duration::from_secs(5))
        .await
        .assignments()
        .await
        .unwrap_err();
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(2800),
        "the call ran past its read budget: {took:?}"
    );
    assert!(matches!(err, MarketError::RetryableRead { .. }), "{err:?}");
}

/// The messages listing (`GET /v1/assignments/{id}/messages`, newest
/// first) returns the full body of a truncated latest message, found by
/// its creation time, and stops paging there (spec `market-client`,
/// Thread read).
#[tokio::test]
async fn the_thread_read_finds_the_truncated_latest_message_and_stops() {
    let server = MockServer::start().await;
    // The optional `senderAgentId` stays absent, so a buyer message has
    // none.
    let m = |id: &str, body: &str, created: &str| {
        instance(
            "ThreadMessage",
            json!({
                "messageId": id,
                "body": body,
                "createdAt": created,
            }),
        )
    };
    // Newest first: today's worker message, then the buyer's truncated
    // one, then an older one the caller stops before.
    let page1 = instance(
        "ListMessagesResponse",
        json!({
            "messages": [
                m("m1", "noted", "2026-11-10T09:00:00Z"),
                m("m2", &"please also check".repeat(600), "2026-11-10T08:00:00Z"),
                m("m3", "old", "2026-11-09T08:00:00Z"),
            ],
            "total": 3,
        }),
    );
    Mock::given(method("GET"))
        .and(path("/v1/assignments/a1/messages"))
        .respond_with(ok_json(page1))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    let until = nearbee_market::wire::wire_time("2026-11-10T08:00:00Z").unwrap();
    let msgs = client_for(&server)
        .await
        .messages("a1", Some(until))
        .await
        .unwrap();
    // The stop page counts: m1 and the full m2, then it stops (m3 never
    // read).
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].message_id, "m1");
    assert_eq!(msgs[1].message_id, "m2");
    assert_eq!(msgs[1].body.len(), "please also check".len() * 600);
    assert!(msgs[1].sender_agent_id.is_none(), "a buyer message");
    // No `until`: the whole page, then it stops (a single page here).
    let msgs = client_for(&server)
        .await
        .messages("a1", None)
        .await
        .unwrap();
    assert_eq!(msgs.len(), 3);

    // Across pages: a full first page whose last item is the stop
    // message; the next page is never requested.
    let server = MockServer::start().await;
    let mut full: Vec<serde_json::Value> = (0..99)
        .map(|i| m(&format!("n{i}"), "newer", "2026-11-10T09:00:00Z"))
        .collect();
    full.push(m("stop", "the change request", "2026-11-10T08:00:00Z"));
    Mock::given(method("GET"))
        .and(path("/v1/assignments/a1/messages"))
        .and(query_param("offset", "0"))
        .respond_with(ok_json(instance(
            "ListMessagesResponse",
            json!({"messages": full, "total": 150}),
        )))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/assignments/a1/messages"))
        .and(query_param("offset", "100"))
        .respond_with(ok_json(instance(
            "ListMessagesResponse",
            json!({"messages": [m("older", "old", "2026-11-09T08:00:00Z")], "total": 150}),
        )))
        .expect(0)
        .mount(&server)
        .await;
    let msgs = client_for(&server)
        .await
        .messages("a1", Some(until))
        .await
        .unwrap();
    assert_eq!(msgs.len(), 100);
    assert_eq!(msgs[99].message_id, "stop");
    server.verify().await;
}

/// The attachment download (spec `market-client`, Attachment download):
/// streams on 200, does not follow a 302, reports 503 with its delay, 403
/// refused, and stops at the cap.
#[tokio::test]
async fn the_download_streams_refuses_redirects_and_honours_the_cap() {
    let server = MockServer::start().await;
    // 200 streamed.
    Mock::given(method("GET"))
        .and(path("/v1/attachments/ok/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; 100]))
        .mount(&server)
        .await;
    let mut out = Vec::new();
    let n = client_for(&server)
        .await
        .download("/v1/attachments/ok/raw", 1000, &mut out)
        .await
        .unwrap();
    assert_eq!(n, 100);
    assert_eq!(out.len(), 100);
    // 302 not followed: a protocol error.
    Mock::given(method("GET"))
        .and(path("/v1/attachments/redir/raw"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/elsewhere", server.uri())),
        )
        .mount(&server)
        .await;
    let mut out = Vec::new();
    let err = client_for(&server)
        .await
        .download("/v1/attachments/redir/raw", 1000, &mut out)
        .await
        .unwrap_err();
    assert!(matches!(err, MarketError::Protocol { .. }), "{err:?}");
    // 503 with Retry-After: a retryable read with the delay.
    Mock::given(method("GET"))
        .and(path("/v1/attachments/scan/raw"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "20"))
        .mount(&server)
        .await;
    let mut out = Vec::new();
    let err = client_for(&server)
        .await
        .download("/v1/attachments/scan/raw", 1000, &mut out)
        .await
        .unwrap_err();
    match err {
        MarketError::RetryableRead {
            retry_after_secs, ..
        } => assert_eq!(retry_after_secs, Some(20)),
        other => panic!("{other:?}"),
    }
    // 403 refused.
    Mock::given(method("GET"))
        .and(path("/v1/attachments/quarantine/raw"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let mut out = Vec::new();
    let err = client_for(&server)
        .await
        .download("/v1/attachments/quarantine/raw", 1000, &mut out)
        .await
        .unwrap_err();
    assert!(matches!(err, MarketError::Refused { .. }), "{err:?}");
    // A body longer than the cap stops reading and errors.
    Mock::given(method("GET"))
        .and(path("/v1/attachments/big/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 5000]))
        .mount(&server)
        .await;
    let mut out = Vec::new();
    let err = client_for(&server)
        .await
        .download("/v1/attachments/big/raw", 1000, &mut out)
        .await
        .unwrap_err();
    assert!(matches!(err, MarketError::Protocol { .. }), "{err:?}");
    // A foreign origin (another server, so its request log can be read)
    // is refused before anything is requested: that server sees nothing.
    let foreign = MockServer::start().await;
    let before = server.received_requests().await.unwrap().len();
    let mut out = Vec::new();
    let err = client_for(&server)
        .await
        .download(&format!("{}/x", foreign.uri()), 1000, &mut out)
        .await
        .unwrap_err();
    assert!(matches!(err, MarketError::Refused { .. }), "{err:?}");
    assert!(
        foreign.received_requests().await.unwrap().is_empty(),
        "a request reached the foreign origin"
    );
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        before,
        "the refused download sent a request"
    );
    // And the bearer travels only on the origin's own requests: every
    // request the origin received carries it.
    let bearer = format!("Bearer {TOKEN}");
    for r in server.received_requests().await.unwrap() {
        assert_eq!(
            r.headers.get("authorization").and_then(|v| v.to_str().ok()),
            Some(bearer.as_str()),
            "{}",
            r.url
        );
    }
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
    let digest = sha256_hex(&bytes);
    Mock::given(method("POST"))
        .and(path("/v1/files/f-1/complete"))
        .and(NoBody)
        .respond_with(ok_json(instance(
            "CompleteFileResponse",
            json!({"fileId": "f-1", "filename": "deliverable.md", "byteSize": 14, "sha256": digest}),
        )))
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
    // What the market stored comes back for the caller to check.
    assert_eq!(
        up.stored,
        Some(StoredFile {
            byte_size: 14,
            sha256: digest,
        })
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    nearbee_core::journal::sha256_hex(bytes)
}

/// Mounts a linked reserve of `f-1` and a PUT answering `put_status`.
async fn reserve_and_put(server: &MockServer, put_status: u16) {
    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(fx::reserve_file_response(
                "f-1",
                &format!("{}/put-here", server.uri()),
                &format!("{}/files/tok/flow.json", server.uri()),
            )),
        )
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/put-here"))
        .respond_with(ResponseTemplate::new(put_status))
        .expect(1)
        .mount(server)
        .await;
}

/// The same-origin upload sink stores the bytes on the PUT, so the complete
/// answers `409 upload_not_pending` (live capture 2026-11-10): the upload
/// is done, with no stored digest reported.
#[tokio::test]
async fn a_complete_answering_upload_not_pending_counts_as_done() {
    let server = MockServer::start().await;
    reserve_and_put(&server, 201).await;
    Mock::given(method("POST"))
        .and(path("/v1/files/f-1/complete"))
        .respond_with(ResponseTemplate::new(409).set_body_json(fx::refusal(
            "upload_not_pending",
            "this upload is already ready",
        )))
        .expect(1)
        .mount(&server)
        .await;
    let up = client_for(&server)
        .await
        .upload_file(&UploadFile {
            name: "flow.json".into(),
            bytes: b"{}".to_vec(),
        })
        .await
        .unwrap();
    assert_eq!(up.file_id, "f-1");
    assert_eq!(up.stored, None);
}

/// A PUT that fails leaves a reserved, unbound file: the client deletes it
/// before it reports the failure (design 6.3).
#[tokio::test]
async fn a_failed_put_deletes_the_reserved_file() {
    let server = MockServer::start().await;
    reserve_and_put(&server, 500).await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/f-1"))
        .and(NoBody)
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let err = client_for(&server)
        .await
        .upload_file(&UploadFile {
            name: "flow.json".into(),
            bytes: b"{}".to_vec(),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, MarketError::WriteOutcomeUnknown { .. }),
        "{err}"
    );
}

/// A complete the market refuses (`400 upload_incomplete`) also deletes the
/// reserved file.
#[tokio::test]
async fn a_refused_complete_deletes_the_reserved_file() {
    let server = MockServer::start().await;
    reserve_and_put(&server, 201).await;
    Mock::given(method("POST"))
        .and(path("/v1/files/f-1/complete"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(fx::refusal("upload_incomplete", "the bytes did not arrive")),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/f-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let err = client_for(&server)
        .await
        .upload_file(&UploadFile {
            name: "flow.json".into(),
            bytes: b"{}".to_vec(),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(&err, MarketError::Refused { code, .. } if code == "upload_incomplete"),
        "{err}"
    );
}

/// The delete: 204 and an unknown file (404) are both gone; a bound file
/// (`409 file_bound`) is a refusal naming its code.
#[tokio::test]
async fn delete_file_answers() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/gone"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/unknown"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "not_found"})))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/bound"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(fx::refusal("file_bound", "part of a job")),
        )
        .mount(&server)
        .await;
    let c = client_for(&server).await;
    c.delete_file("gone").await.unwrap();
    c.delete_file("unknown").await.unwrap();
    let err = c.delete_file("bound").await.unwrap_err();
    assert!(
        matches!(&err, MarketError::Refused { status: 409, code, .. } if code == "file_bound"),
        "{err}"
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

/// The row fields of the spec (`market-client`, Row fields) reach Nearbee's
/// row from a schema-built item: timestamps, SLA seconds, escrow, job id,
/// the job's and the latest message's attachments, and the latest
/// message's sender side, creation time and truncated flag.
#[tokio::test]
async fn the_row_fields_of_the_spec_reach_the_row() {
    let server = MockServer::start().await;
    let assignment = instance_with(
        "JobAssignment",
        &["startedAt", "finalizedAt"],
        json!({
            "assignmentId": "hire-1",
            "status": "accepted",
            "createdAt": "2026-11-09T09:00:00Z",
            "updatedAt": "2026-11-10T09:00:00Z",
            "finalizedAt": "2026-11-11T09:00:00Z",
            "slaSeconds": 3600,
            "escrowAmount": "3.00",
            "escrowToken": "esc-1",
            "slaDeadlineAt": "2026-11-09T11:00:00Z",
        }),
    );
    let att = |id: &str, name: &str, ct: &str, size: i64, sha: &str, url: &str| {
        instance(
            "AttachmentBrief",
            json!({"id": id, "filename": name, "contentType": ct, "byteSize": size,
                "sha256": sha, "downloadUrl": url}),
        )
    };
    let job = instance(
        "WorkerJobBrief",
        json!({
            "jobId": "job-1",
            "title": "Audit",
            "description": "Audit the repository.",
            "attachments": [att("att-job", "spec.md", "text/markdown", 1234, "aa11", "/v1/job/raw")],
        }),
    );
    let latest = instance(
        "LatestMessageBrief",
        json!({
            "senderSide": "buyer",
            "body": "please also check",
            "truncated": true,
            "createdAt": "2026-11-10T08:00:00Z",
            "attachments": [att("att-msg", "notes.pdf", "application/pdf", 5678, "bb22", "/v1/msg/raw")],
        }),
    );
    let item = instance(
        "MyAssignmentItem",
        json!({
            "assignment": assignment,
            "job": job,
            "latestMessage": latest,
        }),
    );
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(fx::assignments(vec![item])))
        .mount(&server)
        .await;
    let rows = client_for(&server).await.assignments().await.unwrap();
    let r = &rows[0];
    assert_eq!(
        r.created_at.map(|t| t.unix_timestamp()),
        nearbee_market::wire::wire_time("2026-11-09T09:00:00Z").map(|t| t.unix_timestamp())
    );
    assert_eq!(
        r.updated_at.map(|t| t.unix_timestamp()),
        nearbee_market::wire::wire_time("2026-11-10T09:00:00Z").map(|t| t.unix_timestamp())
    );
    assert_eq!(
        r.finalized_at.map(|t| t.unix_timestamp()),
        nearbee_market::wire::wire_time("2026-11-11T09:00:00Z").map(|t| t.unix_timestamp())
    );
    assert_eq!(r.sla_seconds, Some(3600));
    assert_eq!(r.escrow_amount.as_deref(), Some("3.00"));
    assert_eq!(r.escrow_token.as_deref(), Some("esc-1"));
    assert_eq!(r.job_id.as_deref(), Some("job-1"));
    // Each attachment travels whole, field by field.
    let expect = |a: &nearbee_core::market::Attachment,
                  id: &str,
                  name: &str,
                  ct: &str,
                  size: i64,
                  sha: &str,
                  url: &str| {
        assert_eq!(
            (
                a.id.as_str(),
                a.filename.as_str(),
                a.content_type.as_str(),
                a.byte_size,
                a.sha256.as_str(),
                a.download_url.as_str()
            ),
            (id, name, ct, size, sha, url)
        );
    };
    assert_eq!(r.job_attachments.len(), 1);
    expect(
        &r.job_attachments[0],
        "att-job",
        "spec.md",
        "text/markdown",
        1234,
        "aa11",
        "/v1/job/raw",
    );
    // The latest message: truncated flag, creation time, attachments.
    let m = r.latest_message.as_ref().unwrap();
    assert!(m.truncated);
    assert_eq!(
        m.created_at.map(|t| t.unix_timestamp()),
        nearbee_market::wire::wire_time("2026-11-10T08:00:00Z").map(|t| t.unix_timestamp())
    );
    assert_eq!(m.attachments.len(), 1);
    expect(
        &m.attachments[0],
        "att-msg",
        "notes.pdf",
        "application/pdf",
        5678,
        "bb22",
        "/v1/msg/raw",
    );
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
    // No bytes moved, and the reservation the answer named is deleted
    // (review 5): no unbound slot outlives the refusal.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[1].method.as_str(), "DELETE");
    assert!(
        requests[1].url.path().starts_with("/v1/files/"),
        "{}",
        requests[1].url
    );
}
