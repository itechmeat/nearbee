//! `nearbee signed-call` against a local `wiremock` server serving the
//! documented and recorded answers (task 2.1 of the change
//! `doc-digest-and-paid-demo`, spec `signed-call`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use nearbee::signed_call::{self, Timing};
use nearbee_cloud::{CloudAttestor, CloudClient};
use nearbee_core::Secret;
use serde_json::Value;
use wiremock::matchers::{body_bytes, header, method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-test-key-0123456789";
const MODEL: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";

fn fixture(name: &str) -> Value {
    let raw = match name {
        "signature" => include_str!("../../nearbee-cloud/tests/fixtures/signature_docs.json"),
        "chat" => include_str!("../../nearbee-cloud/tests/fixtures/chat_completion_docs.json"),
        "report" => include_str!("../../nearbee-cloud/tests/fixtures/attestation_report_live.json"),
        _ => unreachable!(),
    };
    let v: Value = serde_json::from_str(raw).unwrap();
    v["body"].clone()
}

fn sha(b: &[u8]) -> String {
    nearbee_core::journal::sha256_hex(b)
}

/// The signer of the recorded report's only model attestation.
fn signer() -> String {
    fixture("report")["model_attestations"][0]["signing_address"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A request with uneven whitespace, so a re-serialization would change it.
fn request_bytes() -> Vec<u8> {
    format!(
        "{{\"model\": \"{MODEL}\",  \"stream\":false, \"max_tokens\": 8192,\n \"messages\":[{{\"role\":\"user\",\"content\":\"hi\"}}]}}"
    )
    .into_bytes()
}

fn response_bytes(id: &str, finish: &str) -> Vec<u8> {
    let mut b = fixture("chat");
    b["id"] = id.into();
    b["choices"][0]["finish_reason"] = finish.into();
    serde_json::to_vec(&b).unwrap()
}

/// The documented signature answer with its placeholders filled.
fn signature_body(req: &[u8], resp: &[u8], kind: &str, address: &str) -> Value {
    let mut b = fixture("signature");
    let text = b["text"]
        .as_str()
        .unwrap()
        .replace("Qwen/Qwen3.5-122B-A10B", MODEL)
        .replace("{request_hash}", &sha(req))
        .replace("{response_hash}", &sha(resp));
    b["text"] = text.into();
    b["signature"] = format!("0x{}", "1b".repeat(65)).into();
    b["signing_address"] = address.into();
    b["signature_kind"] = kind.into();
    b
}

struct Setup {
    server: MockServer,
    dir: tempfile::TempDir,
    request: PathBuf,
}

impl Setup {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let request = dir.path().join("request.json");
        std::fs::write(&request, request_bytes()).unwrap();
        Self {
            server,
            dir,
            request,
        }
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("signed")
    }

    fn write_request(&self, bytes: &[u8]) {
        std::fs::write(&self.request, bytes).unwrap();
    }

    async fn report(&self, body: Value, n: u64) {
        Mock::given(method("GET"))
            .and(path("/v1/attestation/report"))
            .and(query_param("model", MODEL))
            .and(query_param("provider", "near"))
            .and(query_param("signing_algo", "ecdsa"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(n)
            .mount(&self.server)
            .await;
    }

    async fn chat(&self, t: ResponseTemplate, n: u64) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(t)
            .expect(n)
            .mount(&self.server)
            .await;
    }

    async fn signature(&self, body: Value, n: u64) {
        Mock::given(method("GET"))
            .and(path_regex("^/v1/signature/"))
            .and(query_param("model", MODEL))
            .and(query_param("signing_algo", "ecdsa"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(n)
            .mount(&self.server)
            .await;
    }

    async fn run(&self) -> Result<signed_call::Signed, String> {
        self.run_with(&fast()).await
    }

    async fn run_with(&self, timing: &Timing) -> Result<signed_call::Signed, String> {
        let base = format!("{}/v1", self.server.uri());
        let client = CloudClient::new(base.clone(), Secret::new(KEY)).unwrap();
        let attestor = CloudAttestor::new(base, Secret::new(KEY))
            .unwrap()
            .with_provider("near");
        signed_call::signed_call(
            &client,
            &attestor,
            &self.request,
            &self.out(),
            &[MODEL.to_owned(), "openai/privacy-filter".to_owned()],
            timing,
        )
        .await
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.out().join("state.json")).unwrap()).unwrap()
    }
}

fn fast() -> Timing {
    Timing {
        deadline: Duration::from_secs(30),
        signature_window: Duration::from_secs(5),
        signature_interval: Duration::from_millis(20),
        signature_reserve: Duration::from_millis(100),
    }
}

fn ok_chat(id: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(response_bytes(id, "stop"), "application/json")
}

/// A server where everything succeeds.
async fn happy(s: &Setup) {
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
}

async fn refused_before_sending(bytes: &[u8], needle: &str) {
    let s = Setup::new().await;
    s.write_request(bytes);
    s.report(fixture("report"), 0).await;
    s.chat(ok_chat("chatcmpl-1"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains(needle), "{err}");
}

fn request_with(f: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut v: Value = serde_json::from_slice(&request_bytes()).unwrap();
    f(&mut v);
    serde_json::to_vec(&v).unwrap()
}

#[tokio::test]
async fn a_streaming_request_is_refused_before_sending() {
    refused_before_sending(&request_with(|v| v["stream"] = true.into()), "stream").await;
    refused_before_sending(
        &request_with(|v| {
            v.as_object_mut().unwrap().remove("stream");
        }),
        "stream",
    )
    .await;
}

#[tokio::test]
async fn a_request_without_max_tokens_is_refused() {
    refused_before_sending(
        &request_with(|v| {
            v.as_object_mut().unwrap().remove("max_tokens");
        }),
        "max_tokens",
    )
    .await;
}

#[tokio::test]
async fn max_tokens_above_16384_is_refused() {
    refused_before_sending(&request_with(|v| v["max_tokens"] = 16_385.into()), "16384").await;
}

#[tokio::test]
async fn a_request_over_one_mebibyte_is_refused() {
    let big = request_with(|v| v["pad"] = "x".repeat(1024 * 1024).into());
    refused_before_sending(&big, "1 MiB").await;
}

#[tokio::test]
async fn a_model_not_allowed_is_refused() {
    refused_before_sending(
        &request_with(|v| v["model"] = "Qwen/Qwen3.8-27B".into()),
        "not in the hire's direct models",
    )
    .await;
}

#[tokio::test]
async fn a_direct_model_not_owned_by_nearai_is_refused() {
    let s = Setup::new().await;
    s.write_request(&request_with(|v| {
        v["model"] = "anthropic/claude-sonnet-5".into()
    }));
    let base = format!("{}/v1", s.server.uri());
    let client = CloudClient::new(base.clone(), Secret::new(KEY)).unwrap();
    let attestor = CloudAttestor::new(base, Secret::new(KEY)).unwrap();
    let err = signed_call::signed_call(
        &client,
        &attestor,
        &s.request,
        &s.out(),
        &["anthropic/claude-sonnet-5".to_owned()],
        &fast(),
    )
    .await
    .unwrap_err();
    assert!(err.contains("nearai"), "{err}");
    assert!(s.server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_request_bytes_sent_equal_the_file_bytes() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_bytes(request_bytes()))
        .and(header("x-no-aliasing", "true"))
        .and(header("accept-encoding", "identity"))
        .respond_with(ok_chat("chatcmpl-1"))
        .expect(1)
        .mount(&s.server)
        .await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
    s.run().await.unwrap();
    assert_eq!(
        std::fs::read(s.out().join("request.bin")).unwrap(),
        request_bytes()
    );
}

#[tokio::test]
async fn a_successful_call_writes_the_evidence_files() {
    let s = Setup::new().await;
    happy(&s).await;
    let signed = s.run().await.unwrap();
    assert_eq!(signed.completion_id, "chatcmpl-1");
    assert_eq!(signed.answer.finish_reason, "stop");
    let resp = response_bytes("chatcmpl-1", "stop");
    let out = s.out();
    assert_eq!(std::fs::read(out.join("response.bin")).unwrap(), resp);
    let report: Value =
        serde_json::from_slice(&std::fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report, fixture("report"));
    let sig: Value =
        serde_json::from_slice(&std::fs::read(out.join("signature.json")).unwrap()).unwrap();
    let keys: Vec<&str> = sig
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut want = vec![
        "version",
        "completion_id",
        "model",
        "signature_kind",
        "signing_algo",
        "signing_address",
        "signed_text",
        "signature",
        "request_sha256",
        "response_sha256",
    ];
    let mut got = keys.clone();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want);
    assert_eq!(sig["version"], 1);
    assert_eq!(sig["completion_id"], "chatcmpl-1");
    assert_eq!(sig["model"], MODEL);
    assert_eq!(sig["signature_kind"], "provider_tee");
    assert_eq!(sig["signing_algo"], "ecdsa");
    assert_eq!(sig["signing_address"], signer());
    assert_eq!(sig["request_sha256"], sha(&request_bytes()));
    assert_eq!(sig["response_sha256"], sha(&resp));
    assert_eq!(
        sig["signed_text"],
        format!("{MODEL}:{}:{}", sha(&request_bytes()), sha(&resp))
    );
    assert_eq!(s.state()["phase"], "signed");
}

#[tokio::test]
async fn an_absent_content_encoding_is_accepted_and_identity_too() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(
        ok_chat("chatcmpl-1").insert_header("content-encoding", "identity"),
        1,
    )
    .await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
    s.run().await.unwrap();
}

#[tokio::test]
async fn a_compressed_answer_is_refused() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(
        ok_chat("chatcmpl-1").insert_header("content-encoding", "gzip"),
        1,
    )
    .await;
    s.signature(fixture("signature"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("compressed"), "{err}");
    assert_eq!(s.state()["phase"], "refused");
    assert_eq!(s.state()["reason"], "compressed");
    // A retry fails the same way without a request.
    let err = s.run().await.unwrap_err();
    assert!(err.contains("compressed"), "{err}");
}

#[tokio::test]
async fn a_response_over_four_mebibytes_is_refused() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    let mut big = response_bytes("chatcmpl-1", "stop");
    big.extend(std::iter::repeat_n(b' ', 4 * 1024 * 1024));
    s.chat(
        ResponseTemplate::new(200).set_body_raw(big, "application/json"),
        1,
    )
    .await;
    s.signature(fixture("signature"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("4 MiB"), "{err}");
    assert_eq!(s.state()["phase"], "refused");
    assert!(s.run().await.is_err());
}

#[tokio::test]
async fn an_empty_model_attestations_sends_no_completion() {
    let s = Setup::new().await;
    let mut report = fixture("report");
    report["model_attestations"] = Value::Array(vec![]);
    s.report(report, 1).await;
    s.chat(ok_chat("chatcmpl-1"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("model_attestations"), "{err}");
}

#[tokio::test]
async fn a_report_without_any_signing_address_sends_no_completion() {
    let s = Setup::new().await;
    let mut report = fixture("report");
    for a in report["model_attestations"].as_array_mut().unwrap() {
        a.as_object_mut().unwrap().remove("signing_address");
    }
    s.report(report, 1).await;
    s.chat(ok_chat("chatcmpl-1"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("signing_address"), "{err}");
    assert!(!s.out().join("response.bin").exists());
}

#[tokio::test]
async fn two_instances_sharing_the_signing_key_are_accepted() {
    let s = Setup::new().await;
    let mut report = fixture("report");
    let first = report["model_attestations"][0].clone();
    report["model_attestations"]
        .as_array_mut()
        .unwrap()
        .push(first);
    s.report(report, 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
    s.run().await.unwrap();
    assert!(s.out().join("signature.json").exists());
}

#[tokio::test]
async fn a_gateway_signature_is_refused() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "gateway",
            &signer(),
        ),
        1,
    )
    .await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("not bound to a model TEE"), "{err}");
    assert!(!s.out().join("signature.json").exists());
}

#[tokio::test]
async fn a_signer_not_in_the_preflight_report_is_refused_without_another_report() {
    let s = Setup::new().await;
    // `expect(1)`: no report is fetched after the completion.
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &format!("0x{}", "ab".repeat(20)),
        ),
        1,
    )
    .await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("preflight report"), "{err}");
    assert!(!s.out().join("signature.json").exists());
}

#[tokio::test]
async fn addresses_compare_case_insensitively() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &format!("0x{}", signer()[2..].to_uppercase()),
        ),
        1,
    )
    .await;
    s.run().await.unwrap();
}

#[tokio::test]
async fn a_completion_id_with_a_slash_fails_without_a_signature_fetch() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chat/../x"), 1).await;
    s.signature(fixture("signature"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("completion id"), "{err}");
    assert!(s.run().await.is_err());
}

#[tokio::test]
async fn the_signature_is_accepted_after_two_404s() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    Mock::given(path_regex("^/v1/signature/"))
        .respond_with(ResponseTemplate::new(404))
        .up_to_n_times(2)
        .expect(2)
        .mount(&s.server)
        .await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
    s.run().await.unwrap();
}

#[tokio::test]
async fn the_retry_window_ends_leaving_responded_and_a_resume_fetches_only_the_signature() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    let missing = Mock::given(path_regex("^/v1/signature/"))
        .respond_with(ResponseTemplate::new(404))
        .mount_as_scoped(&s.server)
        .await;
    let timing = Timing {
        signature_window: Duration::from_millis(200),
        ..fast()
    };
    let err = s.run_with(&timing).await.unwrap_err();
    assert!(err.contains("signature"), "{err}");
    assert_eq!(s.state()["phase"], "responded");
    assert_eq!(s.state()["completion_id"], "chatcmpl-1");
    drop(missing);
    // Resume from `responded`: one signature fetch, no completion, no
    // report (the mocks above expect exactly one each overall).
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
    s.run().await.unwrap();
    assert_eq!(s.state()["phase"], "signed");
}

#[tokio::test]
async fn the_deadline_during_signature_retries_leaves_responded() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 1).await;
    Mock::given(path_regex("^/v1/signature/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&s.server)
        .await;
    let timing = Timing {
        deadline: Duration::from_millis(600),
        signature_window: Duration::from_secs(60),
        signature_interval: Duration::from_millis(20),
        signature_reserve: Duration::from_millis(100),
    };
    let started = std::time::Instant::now();
    let err = s.run_with(&timing).await.unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5), "{err}");
    assert_eq!(s.state()["phase"], "responded");
}

/// Prepares `<out>` as an earlier attempt left it in `sending`.
fn leave_sending(out: &Path, with_response: Option<&[u8]>) {
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(out.join("request.bin"), request_bytes()).unwrap();
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec(&fixture("report")).unwrap(),
    )
    .unwrap();
    if let Some(r) = with_response {
        std::fs::write(out.join("response.bin"), r).unwrap();
    }
    std::fs::write(out.join("state.json"), br#"{"phase":"sending"}"#).unwrap();
}

#[tokio::test]
async fn sending_with_a_stored_response_fetches_only_the_signature() {
    let s = Setup::new().await;
    let resp = response_bytes("chatcmpl-1", "stop");
    leave_sending(&s.out(), Some(&resp));
    s.report(fixture("report"), 0).await;
    s.chat(ok_chat("chatcmpl-1"), 0).await;
    s.signature(
        signature_body(&request_bytes(), &resp, "provider_tee", &signer()),
        1,
    )
    .await;
    s.run().await.unwrap();
    assert_eq!(s.state()["phase"], "signed");
}

#[tokio::test]
async fn sending_without_a_stored_response_fails_without_sending() {
    let s = Setup::new().await;
    leave_sending(&s.out(), None);
    s.report(fixture("report"), 0).await;
    s.chat(ok_chat("chatcmpl-1"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("outcome is unknown"), "{err}");
    assert!(err.contains("not resent"), "{err}");
}

#[tokio::test]
async fn a_402_is_refused_and_a_retry_makes_no_request() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ResponseTemplate::new(402), 1).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("402"), "{err}");
    assert!(err.contains("not resent"), "{err}");
    assert_eq!(s.state()["phase"], "refused");
    assert_eq!(s.state()["status"], 402);
    let err = s.run().await.unwrap_err();
    assert!(err.contains("402"), "{err}");
}

#[tokio::test]
async fn a_length_finish_reason_is_refused() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(
        ResponseTemplate::new(200)
            .set_body_raw(response_bytes("chatcmpl-1", "length"), "application/json"),
        1,
    )
    .await;
    s.signature(fixture("signature"), 0).await;
    let err = s.run().await.unwrap_err();
    assert!(err.contains("cut at max_tokens"), "{err}");
    assert_eq!(s.state()["reason"], "truncated");
}

#[tokio::test]
async fn signed_makes_no_call() {
    let s = Setup::new().await;
    happy(&s).await;
    s.run().await.unwrap();
    // The mocks expect one call each overall: a second run calls nothing.
    let again = s.run().await.unwrap();
    assert_eq!(again.completion_id, "chatcmpl-1");
    assert_eq!(s.server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn the_command_fails_with_no_key_before_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    std::fs::write(&request, request_bytes()).unwrap();
    let run_dir = dir.path().join("run");
    std::fs::create_dir(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("run.yaml"),
        format!("params:\n  direct_models: '[\"{MODEL}\"]'\n"),
    )
    .unwrap();
    let run_dir_text = run_dir.to_string_lossy().into_owned();
    let var = move |k: &str| (k == "APB_RUN_DIR").then(|| run_dir_text.clone());
    let env = nearbee::Env {
        cwd: dir.path(),
        var: &var,
        timing: Default::default(),
    };
    let mut out = Vec::new();
    let code =
        nearbee::signed_call_command(&env, &request, &dir.path().join("signed"), &mut out).await;
    assert_eq!(code, nearbee::EXIT_ERROR);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains(nearbee_cloud::NO_KEY), "{text}");
    assert!(!dir.path().join("signed").join("state.json").exists());
}

#[tokio::test]
async fn the_command_refuses_a_missing_direct_models_param() {
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    std::fs::write(&request, request_bytes()).unwrap();
    let run_dir = dir.path().join("run");
    std::fs::create_dir(&run_dir).unwrap();
    std::fs::write(run_dir.join("run.yaml"), "params:\n  brief: x\n").unwrap();
    let run_dir_text = run_dir.to_string_lossy().into_owned();
    let var = move |k: &str| match k {
        "APB_RUN_DIR" => Some(run_dir_text.clone()),
        "NEARAI_API_KEY" => Some(KEY.to_owned()),
        _ => None,
    };
    let env = nearbee::Env {
        cwd: dir.path(),
        var: &var,
        timing: Default::default(),
    };
    let mut out = Vec::new();
    let code =
        nearbee::signed_call_command(&env, &request, &dir.path().join("signed"), &mut out).await;
    assert_eq!(code, nearbee::EXIT_ERROR);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("direct_models"), "{text}");
}

// The completion wait (change `model-thinking-off`, design 2): the time
// left in the invocation's bound minus the signature reserve.

/// A 2 s bound with a 500 ms signature reserve.
fn short_bound() -> Timing {
    Timing {
        deadline: Duration::from_secs(2),
        signature_window: Duration::from_millis(400),
        signature_interval: Duration::from_millis(20),
        signature_reserve: Duration::from_millis(500),
    }
}

#[tokio::test]
async fn a_slow_answer_inside_the_bound_minus_the_reserve_is_signed() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(
        ok_chat("chatcmpl-1").set_delay(Duration::from_millis(1200)),
        1,
    )
    .await;
    s.signature(
        signature_body(
            &request_bytes(),
            &response_bytes("chatcmpl-1", "stop"),
            "provider_tee",
            &signer(),
        ),
        1,
    )
    .await;
    s.run_with(&short_bound()).await.unwrap();
    assert_eq!(s.state()["phase"], "signed");
    assert!(s.out().join("signature.json").exists());
}

#[tokio::test]
async fn an_answer_past_the_bound_minus_the_reserve_fails_and_stays_sending() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1").set_delay(Duration::from_secs(3)), 1)
        .await;
    s.signature(Value::Null, 0).await;
    let started = std::time::Instant::now();
    let err = s.run_with(&short_bound()).await.unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2), "{err}");
    assert_eq!(
        err,
        "the completion did not answer within the invocation's 2 s bound; not resent"
    );
    assert_eq!(s.state()["phase"], "sending");
    assert!(!s.out().join("response.bin").exists());
}

#[tokio::test]
async fn no_time_left_after_the_reserve_fails_before_sending() {
    let s = Setup::new().await;
    s.report(fixture("report"), 1).await;
    s.chat(ok_chat("chatcmpl-1"), 0).await;
    let timing = Timing {
        signature_reserve: Duration::from_secs(5),
        ..short_bound()
    };
    let err = s.run_with(&timing).await.unwrap_err();
    assert_eq!(err, "the invocation reached its 2 s bound");
    assert_eq!(s.state()["phase"], "preflight");
}
