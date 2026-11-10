//! Replay of the live captures of 2026-11-10 (`tests/fixtures/market/`,
//! task 3.6): every recorded answer replays through the client with
//! wiremock and parses into the model types. The fixtures were scrubbed
//! (tokens, capability links, presigned queries, buyer text replaced with
//! synthetic text), so assertions read the recorded shapes, not live ids.

use std::path::PathBuf;
use std::time::Duration;

use nearbee_core::market::{Market, MarketError, RowStatus};
use nearbee_core::secret::Secret;
use nearbee_market::{MarketOptions, NearAgentMarket};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "aat_0123456789abcdef0123456789abcdef";

fn fixture(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/market")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (run the capture example to record the live fixtures)",
            p.display()
        )
    }))
    .unwrap()
}

async fn client(server: &MockServer) -> NearAgentMarket {
    NearAgentMarket::new(MarketOptions {
        base_url: server.uri(),
        token: Secret::new(TOKEN),
        redact: vec![],
        timing: nearbee_market::MarketTiming {
            connect: Duration::from_secs(1),
            read: Duration::from_secs(2),
            write: Duration::from_secs(2),
            read_retry_delays: vec![Duration::ZERO; 3],
            write_resend_delay: Duration::from_millis(10),
            retry_after_ceiling: Duration::from_secs(2),
            read_budget: Duration::from_secs(2),
        },
    })
    .unwrap()
}

fn ok_json(body: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

/// The recorded assignments answer (the S2 test hires: escrow 0, a 600 s
/// SLA, worker-side latest messages) replays and maps onto rows.
#[tokio::test]
async fn the_captured_assignments_replay() {
    let f = fixture("assignments");
    assert_eq!(f["status"], 200, "the capture recorded a success");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ok_json(&f["body"]))
        .mount(&server)
        .await;
    let rows = client(&server).await.assignments().await.unwrap();
    assert!(!rows.is_empty(), "the capture holds the S2 test hires");
    for r in &rows {
        // The live shape the stage documented: zero escrow owner test
        // hires with a 600 s SLA (docs/market.md, live fixture
        // differences).
        assert_eq!(r.sla_seconds, Some(600), "{}", r.id);
        assert_eq!(
            r.escrow_amount.as_deref(),
            Some("0"),
            "{}: an owner test hire is not escrowed",
            r.id
        );
        // Every brief that reaches a row is the scrub's synthetic text.
        assert!(
            r.description
                .as_deref()
                .is_some_and(|d| d.starts_with("Synthetic buyer brief:")),
            "{}: {:?}",
            r.id,
            r.description
        );
        if let Some(m) = &r.latest_message {
            assert!(
                m.body.is_empty() || m.body.starts_with("Synthetic "),
                "{}: {:?}",
                r.id,
                m.body
            );
        }
    }
    // The recorded set carries the S2 lifecycle end states.
    let statuses: Vec<&str> = rows.iter().filter_map(|r| r.status.as_deref()).collect();
    assert!(statuses.contains(&"cancelled"), "{statuses:?}");
    assert!(statuses.contains(&"submitted"), "{statuses:?}");
    // No token survived the scrub.
    assert!(!f.to_string().contains("aat_"), "a token is in the fixture");
}

/// The recorded pricing-plans answer (an empty list, 2026-11-10) replays.
#[tokio::test]
async fn the_captured_pricing_plans_replay() {
    let f = fixture("pricing-plans");
    assert_eq!(f["status"], 200);
    assert_eq!(
        f["body"].as_array().map(Vec::len),
        Some(0),
        "the agent had no pricing plan at capture time"
    );
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/agent-1/pricing-plans"))
        .respond_with(ok_json(&f["body"]))
        .mount(&server)
        .await;
    let plans = client(&server)
        .await
        .pricing_plans("agent-1")
        .await
        .unwrap();
    assert!(plans.is_empty());
}

/// The recorded agent answer (a draft agent: 404) replays as a refusal
/// naming the status.
#[tokio::test]
async fn the_captured_agent_read_replays_as_a_refusal() {
    let f = fixture("agent");
    assert_eq!(f["status"], 404, "a draft agent is not readable");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/repo-audit-fast"))
        .respond_with(ResponseTemplate::new(404).set_body_json(&f["body"]))
        .mount(&server)
        .await;
    let err = client(&server)
        .await
        .agent("repo-audit-fast")
        .await
        .unwrap_err();
    match err {
        MarketError::Refused { status, .. } => assert_eq!(status, 404),
        other => panic!("{other:?}"),
    }
}

/// The recorded reserve answer of the file trio (scrubbed of its
/// presigned queries) parses into the `ReserveFileResponse` the client
/// reads; the trio's statuses are documented in `docs/market.md`.
#[test]
fn the_captured_reserve_answer_parses() {
    let t = fixture("file-trio");
    assert_eq!(t[0]["step"], "reserve");
    let reserve: nearbee_market::wire::ReserveFileResponse =
        serde_json::from_str(t[0]["body"].as_str().unwrap()).expect("the reserve body parses");
    assert!(!reserve.file_id.is_empty());
    assert!(reserve.file_url.is_some());
}

/// The recorded row statuses are exactly the ones the market's enum
/// knows: a live answer with a new status would fail here and force the
/// mapping to be revisited.
#[test]
fn the_captured_statuses_are_known_words() {
    let f = fixture("assignments");
    for it in f["body"]["assignments"].as_array().unwrap() {
        let s = it["assignment"]["status"].as_str().unwrap();
        let parsed = RowStatus::parse(s);
        assert!(!matches!(parsed, RowStatus::Unknown(_)), "{s}");
    }
}
