//! `CloudAttestor` against a local `wiremock` server (task 7.1): the
//! request, the redirect and size rules, the retry and the bounds, and
//! that the key never shows in a failure.

use std::time::{Duration, Instant};

use nearbee_cloud::CloudAttestor;
use nearbee_core::Secret;
use nearbee_core::evidence::Attestor;
use serde_json::Value;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-test-key-0123456789";
const MODEL: &str = "Qwen/Qwen3.8-27B";

fn nonce() -> String {
    "0f".repeat(32)
}

/// The recorded live report body (sanitized), replayed as the success
/// answer.
fn live_body() -> Value {
    let raw = include_str!("fixtures/attestation_report_live.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    v["body"].clone()
}

fn attestor(server: &MockServer) -> CloudAttestor {
    CloudAttestor::new(format!("{}/v1", server.uri()), Secret::new(KEY)).unwrap()
}

#[tokio::test]
async fn the_request_carries_path_query_and_bearer_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/attestation/report"))
        .and(query_param("model", MODEL))
        .and(query_param("signing_algo", "ecdsa"))
        .and(query_param("nonce", nonce()))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(live_body()))
        .expect(1)
        .mount(&server)
        .await;
    let body = attestor(&server).report(MODEL, &nonce()).await.unwrap();
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed, live_body());
    // The model id is URL-encoded on the wire (`/` as `%2F`).
    let requests = server.received_requests().await.unwrap();
    let query = requests[0].url.query().unwrap().to_owned();
    assert!(query.contains("model=Qwen%2FQwen3.8-27B"), "{query}");
    let sent_nonce = requests[0]
        .url
        .query_pairs()
        .find(|(k, _)| k == "nonce")
        .unwrap()
        .1
        .to_string();
    assert_eq!(sent_nonce.len(), 64);
    assert!(sent_nonce.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn a_redirect_is_a_failure_and_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("location", format!("{}/elsewhere", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/elsewhere"))
        .respond_with(ResponseTemplate::new(200).set_body_json(live_body()))
        .expect(0)
        .mount(&server)
        .await;
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    assert!(err.contains("301"), "{err}");
}

#[tokio::test]
async fn a_body_over_two_mebibytes_fails_at_the_cap() {
    let server = MockServer::start().await;
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(3 * 1024 * 1024));
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(200).set_body_string(big))
        .mount(&server)
        .await;
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    assert!(err.contains("2 MiB"), "{err}");
}

#[tokio::test]
async fn a_503_then_a_200_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(200).set_body_json(live_body()))
        .expect(1)
        .mount(&server)
        .await;
    assert!(attestor(&server).report(MODEL, &nonce()).await.is_ok());
}

#[tokio::test]
async fn two_503s_fail() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&server)
        .await;
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    assert!(err.contains("503"), "{err}");
}

#[tokio::test]
async fn a_non_object_body_and_a_4xx_fail_without_a_retry() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[1,2]"))
        .expect(1)
        .mount(&server)
        .await;
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    assert!(err.contains("not a JSON object"), "{err}");

    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    assert!(err.contains("404"), "{err}");
}

/// Both attempts hang: the whole fetch ends inside its 20-second bound
/// (two 9-second attempts and the 1-second wait). This test takes about
/// 19 seconds.
#[tokio::test]
async fn the_whole_fetch_stays_within_twenty_seconds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(live_body())
                .set_delay(Duration::from_secs(60)),
        )
        .mount(&server)
        .await;
    let started = Instant::now();
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    let took = started.elapsed();
    assert!(took <= Duration::from_millis(20_500), "{took:?}");
    assert!(
        took >= Duration::from_secs(18),
        "only one attempt ran: {took:?}"
    );
    assert!(err.contains("timed out"), "{err}");
}

#[tokio::test]
async fn the_key_never_appears_in_an_error() {
    let server = MockServer::start().await;
    // The server echoes the key back in a failure body and a header.
    Mock::given(path("/v1/attestation/report"))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("www-authenticate", format!("Bearer {KEY}").as_str())
                .set_body_string(format!("bad key {KEY}")),
        )
        .mount(&server)
        .await;
    let err = attestor(&server).report(MODEL, &nonce()).await.unwrap_err();
    assert!(!err.contains(KEY), "{err}");
    assert!(err.contains("401"), "{err}");
    // A refused connection (nothing listens) names no key either.
    let closed = CloudAttestor::new("http://127.0.0.1:9/v1", Secret::new(KEY)).unwrap();
    let err = closed.report(MODEL, &nonce()).await.unwrap_err();
    assert!(!err.contains(KEY), "{err}");
}

#[tokio::test]
async fn an_empty_key_fails_without_a_request() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(200).set_body_json(live_body()))
        .expect(0)
        .mount(&server)
        .await;
    let a = CloudAttestor::new(format!("{}/v1", server.uri()), Secret::new("")).unwrap();
    assert_eq!(
        a.report(MODEL, &nonce()).await.unwrap_err(),
        nearbee_cloud::NO_KEY
    );
}
