//! `CloudClient` against a local `wiremock` server serving the documented
//! answers of `tests/fixtures/*_docs.json` (task 1.1 of the change
//! `doc-digest-and-paid-demo`): headers, redirects, proxy, body caps,
//! statuses, the unavailable signature, the classify span text, the
//! completion's `finish_reason`, unknown and missing fields, and the key.

use std::process::Command;

use nearbee_cloud::{CloudAttestor, CloudClient, CloudError};
use nearbee_core::Secret;
use nearbee_core::evidence::Attestor;
use serde_json::Value;
use wiremock::matchers::{body_bytes, body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-test-key-0123456789";

fn fixture(name: &str) -> Value {
    let raw = match name {
        "signature" => include_str!("fixtures/signature_docs.json"),
        "chat" => include_str!("fixtures/chat_completion_docs.json"),
        "classify" => include_str!("fixtures/classify_docs.json"),
        "report" => include_str!("fixtures/attestation_report_live.json"),
        _ => unreachable!(),
    };
    let v: Value = serde_json::from_str(raw).unwrap();
    v["body"].clone()
}

fn client(server: &MockServer) -> CloudClient {
    CloudClient::new(format!("{}/v1", server.uri()), Secret::new(KEY)).unwrap()
}

/// A JSON completion request for the model the documented answer names.
fn fixture_req() -> Value {
    serde_json::json!({"model": fixture("chat")["model"].clone()})
}

fn chat_bytes() -> Vec<u8> {
    serde_json::to_vec(&fixture("chat")).unwrap()
}

// chat_exact

#[tokio::test]
async fn chat_exact_sends_the_bytes_unchanged_with_the_headers() {
    let server = MockServer::start().await;
    let body = br#"{ "model":"deepseek-ai/DeepSeek-V4.1-Flash-1017",  "stream":false }"#.to_vec();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_bytes(body.clone()))
        .and(header("content-type", "application/json"))
        .and(header("x-no-aliasing", "true"))
        .and(header("accept-encoding", "identity"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_raw(chat_bytes(), "application/json"))
        .expect(1)
        .mount(&server)
        .await;
    let raw = client(&server).chat_exact(&body).await.unwrap();
    assert_eq!(raw.status, 200);
    assert_eq!(raw.bytes, chat_bytes());
    assert_eq!(raw.content_encoding, None);
}

#[tokio::test]
async fn chat_exact_returns_a_non_2xx_with_its_status() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(402).set_body_string("no credits"))
        .expect(1)
        .mount(&server)
        .await;
    let raw = client(&server).chat_exact(b"{}").await.unwrap();
    assert_eq!(raw.status, 402);
    assert_eq!(raw.bytes, b"no credits");
}

#[tokio::test]
async fn chat_exact_reports_the_content_encoding() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-encoding", "gzip")
                .set_body_raw(chat_bytes(), "application/json"),
        )
        .mount(&server)
        .await;
    let raw = client(&server).chat_exact(b"{}").await.unwrap();
    assert_eq!(raw.content_encoding.as_deref(), Some("gzip"));
}

#[tokio::test]
async fn chat_exact_does_not_follow_a_redirect() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", format!("{}/x", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(chat_bytes(), "application/json"))
        .expect(0)
        .mount(&server)
        .await;
    let raw = client(&server).chat_exact(b"{}").await.unwrap();
    assert_eq!(raw.status, 307);
}

#[tokio::test]
async fn chat_exact_refuses_a_body_over_four_mebibytes() {
    let server = MockServer::start().await;
    let mut big = chat_bytes();
    big.extend(std::iter::repeat_n(b' ', 4 * 1024 * 1024));
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(big, "application/json"))
        .mount(&server)
        .await;
    let err = client(&server).chat_exact(b"{}").await.unwrap_err();
    assert!(matches!(err, CloudError::TooLarge(_)), "{err}");
    assert!(err.to_string().contains("4 MiB"), "{err}");
}

// signature

#[tokio::test]
async fn signature_sends_model_and_algorithm_and_parses_the_answer() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/signature/f47b4647911249f7a94468f6c2b95832"))
        .and(query_param("model", "Qwen/Qwen3.5-122B-A10B"))
        .and(query_param("signing_algo", "ecdsa"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("signature")))
        .expect(1)
        .mount(&server)
        .await;
    let s = client(&server)
        .signature("f47b4647911249f7a94468f6c2b95832", "Qwen/Qwen3.5-122B-A10B")
        .await
        .unwrap();
    assert_eq!(
        s.text,
        "Qwen/Qwen3.5-122B-A10B:{request_hash}:{response_hash}"
    );
    assert_eq!(s.signature, "0x...");
    assert_eq!(s.signing_address, "0x...");
    assert_eq!(s.signing_algo, "ecdsa");
    assert_eq!(s.signature_kind.as_deref(), Some("provider_tee"));
}

#[tokio::test]
async fn a_404_signature_is_not_yet() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/signature/abc"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let err = client(&server).signature("abc", "m/x").await.unwrap_err();
    assert!(matches!(err, CloudError::NotYet), "{err}");
}

#[tokio::test]
async fn a_2xx_signature_with_an_error_code_is_not_yet() {
    let server = MockServer::start().await;
    let mut body = fixture("signature");
    body["error_code"] = "chat_not_found".into();
    body["message"] = "Chat id not found or expired".into();
    Mock::given(path("/v1/signature/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let err = client(&server).signature("abc", "m/x").await.unwrap_err();
    assert!(matches!(err, CloudError::NotYet), "{err}");
}

#[tokio::test]
async fn a_signature_without_a_required_field_fails_naming_it() {
    let server = MockServer::start().await;
    let mut body = fixture("signature");
    body.as_object_mut().unwrap().remove("signing_address");
    Mock::given(path("/v1/signature/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let err = client(&server).signature("abc", "m/x").await.unwrap_err();
    assert!(err.to_string().contains("signing_address"), "{err}");
}

#[tokio::test]
async fn a_signature_redirect_is_a_status_failure() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/signature/abc"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/x", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("signature")))
        .expect(0)
        .mount(&server)
        .await;
    let err = client(&server).signature("abc", "m/x").await.unwrap_err();
    assert!(matches!(err, CloudError::Status(302)), "{err}");
}

// classify

#[tokio::test]
async fn classify_sends_the_filter_model_and_drops_span_text() {
    let server = MockServer::start().await;
    let input = "My name is John Smith and my email is john@example.com";
    Mock::given(method("POST"))
        .and(path("/v1/privacy/classify"))
        .and(header("x-no-aliasing", "true"))
        .and(body_json(serde_json::json!({
            "model": "openai/privacy-filter",
            "input": input,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("classify")))
        .expect(1)
        .mount(&server)
        .await;
    let spans = client(&server).classify(input).await.unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].category, "private_person");
    assert_eq!((spans[0].start, spans[0].end), (10, 21));
    assert_eq!(spans[1].category, "private_email");
    let shown = format!("{spans:?}");
    assert!(!shown.contains("John"), "{shown}");
    assert!(!shown.contains("john@"), "{shown}");
}

#[tokio::test]
async fn classify_retries_a_5xx_once() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("classify")))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(client(&server).classify("x").await.unwrap().len(), 2);
}

#[tokio::test]
async fn classify_fails_after_two_5xx_and_does_not_retry_a_4xx() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(500))
        .expect(2)
        .mount(&server)
        .await;
    let err = client(&server).classify("x").await.unwrap_err();
    assert!(matches!(err, CloudError::Status(500)), "{err}");

    let server = MockServer::start().await;
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server).classify("x").await.unwrap_err();
    assert!(matches!(err, CloudError::Status(400)), "{err}");
}

#[tokio::test]
async fn classify_without_spans_fails_naming_the_field() {
    let server = MockServer::start().await;
    let mut body = fixture("classify");
    body["data"][0].as_object_mut().unwrap().remove("spans");
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let err = client(&server).classify("x").await.unwrap_err();
    assert!(err.to_string().contains("spans"), "{err}");
}

// chat_json

#[tokio::test]
async fn chat_json_returns_content_finish_reason_and_usage() {
    let server = MockServer::start().await;
    let req = serde_json::json!({"model": "Qwen/Qwen3-VL-30B-A3B-Instruct", "stream": false});
    let mut answer = fixture("chat");
    answer["model"] = "Qwen/Qwen3-VL-30B-A3B-Instruct".into();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_json(req.clone()))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .and(header("x-no-aliasing", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer))
        .expect(1)
        .mount(&server)
        .await;
    let a = client(&server).chat_json(&req).await.unwrap();
    assert!(a.content.starts_with("Hello there!"), "{}", a.content);
    assert_eq!(a.finish_reason, "stop");
    let u = a.usage.unwrap();
    assert_eq!((u.prompt_tokens, u.completion_tokens), (11, 70));
}

#[tokio::test]
async fn chat_json_returns_a_length_finish_reason() {
    let server = MockServer::start().await;
    let mut body = fixture("chat");
    body["choices"][0]["finish_reason"] = "length".into();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let a = client(&server).chat_json(&fixture_req()).await.unwrap();
    assert_eq!(a.finish_reason, "length");
}

#[tokio::test]
async fn chat_json_ignores_unknown_fields_and_names_a_missing_one() {
    let server = MockServer::start().await;
    let mut body = fixture("chat");
    body["system_fingerprint"] = "fp-new".into();
    body["choices"][0]["logprobs"] = Value::Null;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    assert!(client(&server).chat_json(&fixture_req()).await.is_ok());

    let server = MockServer::start().await;
    let mut body = fixture("chat");
    body["choices"][0]
        .as_object_mut()
        .unwrap()
        .remove("finish_reason");
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let err = client(&server).chat_json(&fixture_req()).await.unwrap_err();
    assert!(err.to_string().contains("finish_reason"), "{err}");
}

#[tokio::test]
async fn chat_json_does_not_retry_a_failure() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server).chat_json(&fixture_req()).await.unwrap_err();
    assert!(matches!(err, CloudError::Status(503)), "{err}");
}

#[tokio::test]
async fn chat_json_refuses_a_body_over_the_cap() {
    let server = MockServer::start().await;
    let mut big = chat_bytes();
    big.extend(std::iter::repeat_n(b' ', 4 * 1024 * 1024));
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(big, "application/json"))
        .mount(&server)
        .await;
    let err = client(&server).chat_json(&fixture_req()).await.unwrap_err();
    assert!(matches!(err, CloudError::TooLarge(_)), "{err}");
}

#[tokio::test]
async fn chat_json_refuses_an_answer_from_another_model() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("chat")))
        .expect(1)
        .mount(&server)
        .await;
    // The documented answer names zai-org/GLM-5.1-FP8.
    let err = client(&server)
        .chat_json(&serde_json::json!({"model": "Qwen/Qwen3-VL-30B-A3B-Instruct"}))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("another model"), "{err}");
    let mut no_model = fixture("chat");
    no_model.as_object_mut().unwrap().remove("model");
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(no_model))
        .mount(&server)
        .await;
    let err = client(&server).chat_json(&fixture_req()).await.unwrap_err();
    assert!(err.to_string().contains("another model"), "{err}");
}

#[tokio::test]
async fn classify_refuses_an_answer_from_another_model() {
    let server = MockServer::start().await;
    let mut body = fixture("classify");
    body["model"] = "openai/other-filter".into();
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server).classify("x").await.unwrap_err();
    assert!(err.to_string().contains("another model"), "{err}");
}

// no response content in an error

const SECRET_TEXT: &str = "Invoice for Jane Doe";

#[tokio::test]
async fn a_malformed_answer_error_never_quotes_its_content() {
    // A completion whose fields carry text of the wrong type.
    let server = MockServer::start().await;
    let mut chat = fixture("chat");
    chat["choices"][0]["finish_reason"] = serde_json::json!({"x": SECRET_TEXT});
    chat["usage"]["prompt_tokens"] = SECRET_TEXT.into();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat))
        .mount(&server)
        .await;
    let mut sig = fixture("signature");
    sig["signing_address"] = serde_json::json!([SECRET_TEXT]);
    Mock::given(path("/v1/signature/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sig))
        .mount(&server)
        .await;
    let mut classify = fixture("classify");
    classify["data"][0]["spans"][0]["start"] = SECRET_TEXT.into();
    Mock::given(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(classify))
        .mount(&server)
        .await;
    let c = client(&server);
    let errs = [
        c.chat_json(&fixture_req()).await.unwrap_err().to_string(),
        c.signature("abc", "m/x").await.unwrap_err().to_string(),
        c.classify("x").await.unwrap_err().to_string(),
    ];
    for (e, field) in errs
        .iter()
        .zip(["finish_reason", "signing_address", "start"])
    {
        assert!(!e.contains("Jane"), "{e}");
        assert!(e.contains(field), "{field}: {e}");
    }
    let mut content = fixture("chat");
    content["choices"][0]["message"]["content"] = 42.into();
    content["model"] = SECRET_TEXT.into();
    let raw = serde_json::to_vec(&content).unwrap();
    let e = nearbee_cloud::parse_chat(&raw).unwrap_err().to_string();
    assert!(!e.contains("Jane") && e.contains("content"), "{e}");
    let mut text = br#"{"choices": "#.to_vec();
    text.extend_from_slice(SECRET_TEXT.as_bytes());
    let e = nearbee_cloud::parse_chat(&text).unwrap_err().to_string();
    assert!(!e.contains("Jane"), "{e}");
}

// the key

#[tokio::test]
async fn the_key_never_appears_in_an_error() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("www-authenticate", format!("Bearer {KEY}").as_str())
                .set_body_string(format!("bad key {KEY}")),
        )
        .mount(&server)
        .await;
    let c = client(&server);
    let errs = [
        c.signature("abc", "m/x").await.unwrap_err().to_string(),
        c.classify("x").await.unwrap_err().to_string(),
        c.chat_json(&fixture_req()).await.unwrap_err().to_string(),
    ];
    for e in errs {
        assert!(!e.contains(KEY), "{e}");
        assert!(e.contains("401"), "{e}");
    }
    let closed = CloudClient::new("http://127.0.0.1:9/v1", Secret::new(KEY)).unwrap();
    let e = closed.chat_exact(b"{}").await.unwrap_err().to_string();
    assert!(!e.contains(KEY), "{e}");
    let e = closed
        .chat_json(&fixture_req())
        .await
        .unwrap_err()
        .to_string();
    assert!(!e.contains(KEY), "{e}");
}

// the model report with a provider

#[tokio::test]
async fn the_report_carries_the_provider_when_set() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/attestation/report"))
        .and(query_param("model", "deepseek-ai/DeepSeek-V4.1-Flash-1017"))
        .and(query_param("provider", "near"))
        .and(query_param("signing_algo", "ecdsa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("report")))
        .expect(1)
        .mount(&server)
        .await;
    let a = CloudAttestor::new(format!("{}/v1", server.uri()), Secret::new(KEY))
        .unwrap()
        .with_provider("near");
    a.report("deepseek-ai/DeepSeek-V4.1-Flash-1017", &"0f".repeat(32))
        .await
        .unwrap();
}

// no proxy: the proxy variables are process wide and the workspace forbids
// `unsafe` (`set_var`), so the test runs itself again as a child process
// with the variables set.

const CHILD: &str = "NEARBEE_CLOUD_PROXY_TEST_URL";
const TEST: &str = "system_proxy_settings_are_ignored";

#[tokio::test(flavor = "multi_thread")]
async fn system_proxy_settings_are_ignored() {
    if let Ok(url) = std::env::var(CHILD) {
        let c = CloudClient::new(url, Secret::new(KEY)).unwrap();
        c.chat_json(&fixture_req()).await.unwrap();
        c.chat_exact(b"{}").await.unwrap();
        let a = CloudAttestor::new(std::env::var(CHILD).unwrap(), Secret::new(KEY)).unwrap();
        a.report("m/x", &"0f".repeat(32)).await.unwrap();
        return;
    }
    let cloud = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("chat")))
        .expect(2)
        .mount(&cloud)
        .await;
    Mock::given(path("/v1/attestation/report"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("report")))
        .expect(1)
        .mount(&cloud)
        .await;
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(502))
        .mount(&proxy)
        .await;
    let exe = std::env::current_exe().unwrap();
    let uri = format!("{}/v1", cloud.uri());
    let proxy_uri = proxy.uri();
    let out = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(exe);
        cmd.args([TEST, "--exact", "--nocapture"]).env(CHILD, uri);
        for v in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            cmd.env(v, &proxy_uri);
        }
        for v in ["NO_PROXY", "no_proxy"] {
            cmd.env_remove(v);
        }
        cmd.output().unwrap()
    })
    .await
    .unwrap();
    assert!(
        out.status.success(),
        "child failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        proxy.received_requests().await.unwrap().is_empty(),
        "a request went through the proxy"
    );
}
