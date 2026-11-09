//! The bearer key never reaches an error or a log line. Its own test binary,
//! so no other test shares the process-wide tracing call-site cache.

mod common;

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::*;
use nearbee_apb::ApbRunner;
use nearbee_core::Secret;
use nearbee_core::runner::{RunId, Runner};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Collects log output for the redaction test.
#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn bearer_key_is_sent_and_never_appears_in_errors_or_logs() {
    let key = "apb_TESTKEY0123456789abcdefghijklmnopqrstuvw";
    let buf = Buf::default();
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    // Other tests may have run the log call sites with no subscriber, which
    // caches them as disabled process-wide.
    tracing::callsite::rebuild_interest_cache();

    let server = MockServer::start().await;
    let auth = format!("Bearer {key}");
    let mut f = fixture("projects");
    f["request"]["query"] = json!([]);
    builder(&f, true)
        .and(header("authorization", auth.as_str()))
        .respond_with(response(&f))
        .mount(&server)
        .await;
    let echo = move |r: &Request| {
        let h = r
            .headers
            .get("authorization")
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        ResponseTemplate::new(503).set_body_string(format!("denied {h} key={key}"))
    };
    Mock::given(method("GET"))
        .and(path("/api/runs/r1"))
        .and(header("authorization", auth.as_str()))
        .respond_with(echo)
        .mount(&server)
        .await;
    let echo_text = move |r: &Request| {
        let h = r
            .headers
            .get("authorization")
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        ResponseTemplate::new(400).set_body_string(format!("bad {h} key={key}"))
    };
    Mock::given(method("GET"))
        .and(path("/api/runs/r2"))
        .respond_with(echo_text)
        .mount(&server)
        .await;
    // A JSON refusal that echoes the key in its code and its estimate.
    Mock::given(method("POST"))
        .and(path("/api/playbooks/smoke-qwen/run"))
        .respond_with(move |r: &Request| {
            let h = r
                .headers
                .get("authorization")
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default();
            ResponseTemplate::new(409).set_body_json(json!({
                "error": format!("denied:{key}"),
                "detail": format!("no: {h}"),
                "estimate": {"auth": h, "nested": [format!("key={key}")]},
            }))
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(move |r: &Request| {
            let h = r
                .headers
                .get("authorization")
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default();
            ResponseTemplate::new(422).set_body_string(format!("no: {h} {key}"))
        })
        .mount(&server)
        .await;

    let mut o = opts(&server);
    o.api_key = Some(Secret::new(key));
    let runner = ApbRunner::connect(o, Path::new(WS_PATH)).await.unwrap();
    let mut errors = vec![
        runner.run(&RunId::new("r1")).await.unwrap_err(),
        runner.run(&RunId::new("r2")).await.unwrap_err(),
    ];
    let (req, _, _dir) = start_request(&fixture("start"), "smoke-own");
    errors.push(runner.start(req).await.unwrap_err());
    let (req, _, _dir2) = start_request(&fixture("start"), "smoke-qwen");
    let json_refusal = runner.start(req).await.unwrap_err();
    assert!(
        matches!(
            json_refusal,
            nearbee_core::runner::RunnerError::Refused { status: 409, .. }
        ),
        "{json_refusal:?}"
    );
    errors.push(json_refusal);
    for e in &errors {
        let text = format!("{e} {e:?}");
        assert!(!text.contains(key), "{text}");
        assert!(!text.contains("TESTKEY"), "{text}");
    }
    assert!(format!("{runner:?}").contains("<redacted>"));
    assert!(!format!("{runner:?}").contains(key));
    let logs = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("retrying apb read"),
        "the retry was logged: {logs}"
    );
    assert!(!logs.contains(key) && !logs.contains("TESTKEY"), "{logs}");
    // Every request carried the bearer header: the project list, both reads
    // with their retries, and both starts.
    let received = server.received_requests().await.unwrap();
    assert!(
        received
            .iter()
            .filter(|r| r.method.as_str() == "POST")
            .count()
            == 2,
        "both starts reached the server"
    );
    for r in &received {
        assert_eq!(
            r.headers.get("authorization").map(|v| v.to_str().unwrap()),
            Some(auth.as_str()),
            "{} {}",
            r.method,
            r.url
        );
    }
}
