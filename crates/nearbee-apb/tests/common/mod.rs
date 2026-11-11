//! Helpers shared by the runner tests: recorded apb answers
//! (`tests/fixtures/apb/`) replayed with wiremock.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nearbee_apb::{ApbOptions, ApbRunner, Timing};
use nearbee_core::runner::{Budget, ExecutionId, HireId, OnNoEstimate, PlaybookId, StartRequest};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

pub const WS: &str = "ws-c0188762df734bf4a1f03f8db22f19d7";
pub const OTHER_WS: &str = "ws-6f506366373b4c6ea2809fdd15552cba";
pub const WS_PATH: &str = "<home>/.local/share/nearbee/apb-workspace";

pub fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/apb")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

pub fn response(f: &Value) -> ResponseTemplate {
    let r = &f["response"];
    let status = r["status"].as_u64().unwrap() as u16;
    match &r["body"] {
        Value::String(s) => ResponseTemplate::new(status)
            .insert_header("content-type", "text/plain; charset=utf-8")
            .set_body_string(s.clone()),
        other => ResponseTemplate::new(status).set_body_json(other.clone()),
    }
}

/// A mock for the recorded request: method, path, every query pair, and the
/// body unless `match_body` is false.
pub fn recorded(f: &Value, match_body: bool) -> Mock {
    recorded_with(f, match_body, response(f))
}

pub fn recorded_with(f: &Value, match_body: bool, resp: ResponseTemplate) -> Mock {
    builder(f, match_body).respond_with(resp)
}

pub fn builder(f: &Value, match_body: bool) -> MockBuilder {
    let req = &f["request"];
    let mut m = Mock::given(method(req["method"].as_str().unwrap()))
        .and(path(req["path"].as_str().unwrap()));
    for pair in req["query"].as_array().unwrap() {
        m = m.and(query_param(
            pair[0].as_str().unwrap(),
            pair[1].as_str().unwrap(),
        ));
    }
    if match_body && !req["body"].is_null() {
        m = m.and(body_json(req["body"].clone()));
    }
    m
}

pub fn fast_timing() -> Timing {
    Timing {
        connect: Duration::from_secs(1),
        read: Duration::from_secs(2),
        estimate: Duration::from_secs(2),
        start: Duration::from_millis(500),
        abort: Duration::from_secs(5),
        decide: Duration::from_secs(5),
        read_retry_delays: vec![Duration::ZERO; 3],
    }
}

pub fn opts(server: &MockServer) -> ApbOptions {
    ApbOptions {
        url: server.uri(),
        api_key: None,
        redact: vec![],
        timing: fast_timing(),
        bin: PathBuf::from("apb"),
        config_dir: PathBuf::from("/tmp/nearbee-test-apb-config"),
        systemctl_bin: PathBuf::from("systemctl"),
        xdg_config_home: PathBuf::from("/nonexistent/xdg-config"),
    }
}

pub async fn connected(server: &MockServer) -> ApbRunner {
    recorded(&fixture("projects"), true).mount(server).await;
    ApbRunner::connect(opts(server), Path::new(WS_PATH))
        .await
        .unwrap()
}

/// The start request of the `start` fixture, with its worktree replaced by a
/// real directory; returns the request and the body the runner must send.
pub fn start_request(f: &Value, playbook: &str) -> (StartRequest, Value, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let wt = dir.path().canonicalize().unwrap();
    let mut body = f["request"]["body"].clone();
    body["worktree"] = json!(wt.to_string_lossy());
    let params = body["params"].as_object().unwrap();
    let mut caller: BTreeMap<String, String> = params
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
        .collect();
    let hire = caller.remove("hire_id").unwrap();
    let exec = caller.remove("execution_id").unwrap();
    let budget = &body["budget"];
    let usd = budget["usd"]
        .as_f64()
        .map(|f| Decimal::from_str_exact(&f.to_string()).unwrap());
    let on_no_estimate = match budget["on_no_estimate"].as_str() {
        Some("continue") => OnNoEstimate::Continue,
        Some("pause") => OnNoEstimate::Pause,
        _ => OnNoEstimate::Ask,
    };
    let req = StartRequest {
        playbook: PlaybookId::new(playbook),
        instruction: body["instruction"].as_str().unwrap().to_owned(),
        params: caller,
        worktree: PathBuf::from(&wt),
        hire_id: HireId::new(hire),
        execution_id: ExecutionId::new(exec),
        budget: usd.map(|_| Budget {
            usd,
            on_no_estimate,
        }),
        continued_from: None,
    };
    (req, body, dir)
}

/// What a [`RawServer`] does with one request.
pub enum Reply {
    /// Write these bytes, then close the connection.
    Bytes(Vec<u8>),
    /// Close the connection without answering.
    Drop,
}

/// A plain TCP HTTP server for answers wiremock cannot give (a body cut
/// short, a connection dropped after the request). It keeps its port for
/// the whole test, so no other server can take it. Each connection carries
/// one request; `handler` gets the request line and the number of earlier
/// requests with the same request line.
pub struct RawServer {
    pub url: String,
    pub seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl RawServer {
    pub fn start(handler: impl Fn(&str, usize) -> Reply + Send + 'static) -> Self {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let request_line = line.trim_end().to_owned();
                let mut length = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':')
                        && k.eq_ignore_ascii_case("content-length")
                    {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let n = {
                    let mut seen = log.lock().unwrap();
                    let n = seen.iter().filter(|s| **s == request_line).count();
                    seen.push(request_line.clone());
                    n
                };
                match handler(&request_line, n) {
                    Reply::Bytes(b) => {
                        let _ = conn.write_all(&b);
                    }
                    Reply::Drop => {}
                }
            }
        });
        Self { url, seen }
    }

    /// How many requests started with `prefix` (for example `GET /api/runs/r1`).
    pub fn count(&self, prefix: &str) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with(prefix))
            .count()
    }
}

/// A complete `200 OK` JSON answer that closes the connection.
pub fn ok_json(body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// Options for a [`RawServer`] with no key and fast timing.
pub fn raw_opts(server: &RawServer) -> ApbOptions {
    ApbOptions {
        url: server.url.clone(),
        api_key: None,
        redact: vec![],
        timing: fast_timing(),
        bin: PathBuf::from("apb"),
        config_dir: PathBuf::from("/tmp/nearbee-test-apb-config"),
        systemctl_bin: PathBuf::from("systemctl"),
        xdg_config_home: PathBuf::from("/nonexistent/xdg-config"),
    }
}
