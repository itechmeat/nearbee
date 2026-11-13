//! Agent B's commands (tasks 4.1 to 4.3 of the change
//! `doc-digest-and-paid-demo`, spec `doc-digest`): `pages` with generated
//! PDF and image fixtures and the real tools, `extract`, `classify` and
//! `compose` against a local `wiremock` server serving the documented
//! answers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nearbee::digest::{self, PageText, PagesFile, Tools};
use nearbee::signed_call::Timing;
use nearbee_cloud::{CloudAttestor, CloudClient};
use nearbee_core::Secret;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const KEY: &str = "sk-test-key-0123456789";
const EXEC: &str = "hire-a1-1";

fn fixture(name: &str) -> Value {
    let raw = match name {
        "signature" => include_str!("../../nearbee-cloud/tests/fixtures/signature_docs.json"),
        "chat" => include_str!("../../nearbee-cloud/tests/fixtures/chat_completion_docs.json"),
        "classify" => include_str!("../../nearbee-cloud/tests/fixtures/classify_docs.json"),
        "report" => include_str!("../../nearbee-cloud/tests/fixtures/attestation_report_live.json"),
        _ => unreachable!(),
    };
    let v: Value = serde_json::from_str(raw).unwrap();
    v["body"].clone()
}

fn all_models() -> String {
    serde_json::to_string(&[
        digest::VISION_MODEL,
        digest::CLASSIFY_MODEL,
        digest::DIGEST_MODEL,
    ])
    .unwrap()
}

fn params(exec: &str, inputs: &[&str]) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("execution_id".into(), exec.into()),
        ("inputs".into(), serde_json::to_string(inputs).unwrap()),
        ("direct_models".into(), all_models()),
        ("feedback".into(), String::new()),
    ])
}

// ---------------------------------------------------------------------------
// Fixtures for `pages`
// ---------------------------------------------------------------------------

/// The tools, or `None` with a message when one is missing (the test is
/// then skipped).
fn tools() -> Option<Tools> {
    match Tools::resolve(std::env::var("PATH").ok().as_deref()) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("skipped: {e} (install poppler-utils and ImageMagick 7)");
            None
        }
    }
}

/// A minimal valid PDF of `pages` blank pages of 200 x 300 points; with
/// `encrypted`, a Standard security handler with a password nobody knows.
fn pdf(pages: usize, encrypted: bool) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    for _ in 0..pages {
        objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>".into());
    }
    if encrypted {
        objects.push(format!(
            "<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P -4 >>",
            "ab".repeat(32),
            "cd".repeat(32)
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    let encrypt = if encrypted {
        format!(
            " /Encrypt {} 0 R /ID [<{}> <{}>]",
            objects.len(),
            "11".repeat(16),
            "11".repeat(16)
        )
    } else {
        String::new()
    };
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{encrypt} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

/// A PNG or JPEG of `w` x `h` made by ImageMagick.
fn image(tools: &Tools, dir: &Path, name: &str, w: u32, h: u32) -> Vec<u8> {
    let p = dir.join(name);
    let status = std::process::Command::new(&tools.magick)
        .args(["-size", &format!("{w}x{h}"), "xc:white"])
        .arg(&p)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(&p).unwrap()
}

fn dims(tools: &Tools, p: &Path) -> (u32, u32) {
    let out = std::process::Command::new(&tools.magick)
        .args(["identify", "-format", "%w %h"])
        .arg(p)
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let mut it = text.split_whitespace().map(|n| n.parse::<u32>().unwrap());
    (it.next().unwrap(), it.next().unwrap())
}

struct Work {
    dir: tempfile::TempDir,
}

impl Work {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("inputs")).unwrap();
        Self { dir }
    }

    fn cwd(&self) -> &Path {
        self.dir.path()
    }

    fn input(&self, name: &str, bytes: &[u8]) -> String {
        std::fs::write(self.cwd().join("inputs").join(name), bytes).unwrap();
        format!("inputs/{name}")
    }

    fn exec(&self, id: &str) -> PathBuf {
        self.cwd().join("digest").join(id)
    }
}

// ---------------------------------------------------------------------------
// 4.1 pages
// ---------------------------------------------------------------------------

#[test]
fn pages_sniff_render_and_cap_an_image_at_two_megapixels() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    let tmp = tempfile::tempdir().unwrap();
    let big = image(&t, tmp.path(), "big.png", 3000, 2600);
    let jpeg = image(&t, tmp.path(), "small.jpg", 800, 600);
    // Sniffed by content, not by name: a PNG named .pdf is an image.
    let a = w.input("01-scan.pdf", &big);
    let b = w.input("02-photo.jpg", &jpeg);
    let c = w.input("03-notes.txt", b"plain text");
    let doc = w.input("04-doc.pdf", &pdf(2, false));
    let p = digest::pages(w.cwd(), &params(EXEC, &[&a, &b, &c, &doc]), &t).unwrap();
    assert_eq!(p.pages.len(), 4);
    assert_eq!(p.unusable, 1, "the text file");
    assert_eq!(p.skipped, 0);
    assert_eq!(p.pages[3].source, doc);
    assert_eq!(p.pages[3].source_page, 2);
    let pages = w.exec(EXEC).join("pages");
    for n in 1..=4 {
        let (x, y) = dims(&t, &pages.join(format!("{n}.png")));
        assert!(x.max(y) <= 2000, "page {n}: {x}x{y}");
        assert!(
            u64::from(x) * u64::from(y) <= 2_000_000,
            "page {n}: {x}x{y}"
        );
    }
    // State lives under digest/<execution_id>/.
    assert!(w.exec(EXEC).join("pages.json").exists());
    assert!(w.exec(EXEC).join("src").is_dir());
    assert!(!w.cwd().join("pages.json").exists());
}

#[test]
fn fifty_pages_render_forty_and_count_ten_skipped() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    let doc = w.input("01-long.pdf", &pdf(50, false));
    let p = digest::pages(w.cwd(), &params(EXEC, &[&doc]), &t).unwrap();
    assert_eq!(p.pages.len(), 40);
    assert_eq!(p.skipped, 10);
    let stored: PagesFile =
        serde_json::from_slice(&std::fs::read(w.exec(EXEC).join("pages.json")).unwrap()).unwrap();
    assert_eq!(stored, p);
}

#[test]
fn an_encrypted_pdf_is_skipped_and_counted() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    let tmp = tempfile::tempdir().unwrap();
    let png = image(&t, tmp.path(), "a.png", 100, 100);
    let locked = w.input("01-locked.pdf", &pdf(1, true));
    let ok = w.input("02-a.png", &png);
    let p = digest::pages(w.cwd(), &params(EXEC, &[&locked, &ok]), &t).unwrap();
    assert_eq!(p.unusable, 1);
    assert_eq!(p.pages.len(), 1);
}

#[test]
fn eleven_files_are_refused() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    let names: Vec<String> = (0..11)
        .map(|i| w.input(&format!("{i:02}.txt"), b"x"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let err = digest::pages(w.cwd(), &params(EXEC, &refs), &t).unwrap_err();
    assert!(err.contains("11 files"), "{err}");
}

#[test]
fn no_usable_file_fails_with_the_message() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    let txt = w.input("01-notes.txt", b"just text");
    let err = digest::pages(w.cwd(), &params(EXEC, &[&txt]), &t).unwrap_err();
    assert_eq!(err, digest::NO_USABLE_FILE);
    let err = digest::pages(w.cwd(), &params(EXEC, &[]), &t).unwrap_err();
    assert_eq!(err, digest::NO_USABLE_FILE);
    assert!(!w.exec(EXEC).join("pages.json").exists());
}

#[test]
fn files_over_25_mib_in_total_are_refused() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    let chunk = vec![0u8; 13 * 1024 * 1024];
    let a = w.input("01-a.bin", &chunk);
    let b = w.input("02-b.bin", &chunk);
    let err = digest::pages(w.cwd(), &params(EXEC, &[&a, &b]), &t).unwrap_err();
    assert!(err.contains("25 MiB"), "{err}");
}

#[test]
fn a_path_outside_inputs_and_a_symlink_are_refused() {
    let Some(t) = tools() else { return };
    let w = Work::new();
    std::fs::write(w.cwd().join("secret.pdf"), pdf(1, false)).unwrap();
    let err = digest::pages(w.cwd(), &params(EXEC, &["../secret.pdf"]), &t).unwrap_err();
    assert!(err.contains("not a path under inputs/"), "{err}");
    let err = digest::pages(w.cwd(), &params(EXEC, &["secret.pdf"]), &t).unwrap_err();
    assert!(err.contains("not a path under inputs/"), "{err}");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            w.cwd().join("secret.pdf"),
            w.cwd().join("inputs/01-link.pdf"),
        )
        .unwrap();
        let err = digest::pages(w.cwd(), &params(EXEC, &["inputs/01-link.pdf"]), &t).unwrap_err();
        assert!(err.contains("refused"), "{err}");
        assert!(!w.exec(EXEC).join("src/01.pdf").exists(), "never copied");
    }
}

#[test]
fn a_missing_or_malformed_execution_id_is_refused() {
    let w = Work::new();
    let t = Tools {
        prlimit: "/nonexistent".into(),
        pdfinfo: "/nonexistent".into(),
        pdftoppm: "/nonexistent".into(),
        magick: "/nonexistent".into(),
    };
    let mut p = params(EXEC, &[]);
    p.remove("execution_id");
    let err = digest::pages(w.cwd(), &p, &t).unwrap_err();
    assert!(err.contains("execution_id is missing"), "{err}");
    for bad in ["../x", "a/b", "", "a b"] {
        let err = digest::pages(w.cwd(), &params(bad, &[]), &t).unwrap_err();
        assert!(err.contains("execution_id is not"), "{bad}: {err}");
    }
    assert!(!w.cwd().join("digest").exists());
}

// ---------------------------------------------------------------------------
// Fixtures for the model steps
// ---------------------------------------------------------------------------

/// An execution directory as `pages` leaves it, with `n` page images.
fn rendered(w: &Work, exec: &str, n: usize, skipped: usize) {
    let dir = w.exec(exec);
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    let pages: Vec<Value> = (1..=n)
        .map(|k| json!({"page": k, "source": "inputs/01-a.pdf", "source_page": k}))
        .collect();
    std::fs::write(
        dir.join("pages.json"),
        serde_json::to_vec(
            &json!({"version": 1, "pages": pages, "skipped": skipped, "unusable": 0}),
        )
        .unwrap(),
    )
    .unwrap();
    for k in 1..=n {
        std::fs::write(
            dir.join("pages").join(format!("{k}.png")),
            format!("\u{89}PNG page {k}"),
        )
        .unwrap();
    }
}

fn store_text(w: &Work, exec: &str, page: usize, text: &str, truncated: bool) {
    std::fs::write(
        w.exec(exec).join("pages").join(format!("{page}.json")),
        serde_json::to_vec(&PageText {
            page,
            text: text.into(),
            truncated,
        })
        .unwrap(),
    )
    .unwrap();
}

/// The documented completion answer with `content` and `finish`, from
/// the vision model.
fn chat_answer(content: &str, finish: &str) -> Vec<u8> {
    chat_answer_from(digest::VISION_MODEL, content, finish)
}

/// The documented completion answer naming `model`.
fn chat_answer_from(model: &str, content: &str, finish: &str) -> Vec<u8> {
    let mut b = fixture("chat");
    b["id"] = "chatcmpl-digest-1".into();
    b["model"] = model.into();
    b["choices"][0]["message"]["content"] = content.into();
    b["choices"][0]["finish_reason"] = finish.into();
    serde_json::to_vec(&b).unwrap()
}

fn client(server: &MockServer) -> CloudClient {
    CloudClient::new(format!("{}/v1", server.uri()), Secret::new(KEY)).unwrap()
}

fn attestor(server: &MockServer) -> CloudAttestor {
    CloudAttestor::new(format!("{}/v1", server.uri()), Secret::new(KEY))
        .unwrap()
        .with_provider("near")
}

fn fast() -> Timing {
    Timing {
        deadline: Duration::from_secs(30),
        signature_window: Duration::from_secs(5),
        signature_interval: Duration::from_millis(20),
        signature_reserve: Duration::from_millis(100),
    }
}

fn bodies(reqs: &[Request], p: &str) -> Vec<Value> {
    reqs.iter()
        .filter(|r| r.url.path() == p)
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

// ---------------------------------------------------------------------------
// 4.2 extract and classify
// ---------------------------------------------------------------------------

#[tokio::test]
async fn extract_sends_the_page_as_an_image_part_and_resumes() {
    let server = MockServer::start().await;
    let w = Work::new();
    rendered(&w, EXEC, 3, 0);
    store_text(&w, EXEC, 1, "already", false);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(chat_answer("Invoice 42", "stop"), "application/json"),
        )
        .expect(2)
        .mount(&server)
        .await;
    let n = digest::extract(w.cwd(), &params(EXEC, &[]), &client(&server))
        .await
        .unwrap();
    assert_eq!(n, 2, "page 1 was stored before");
    let reqs = server.received_requests().await.unwrap();
    let sent = bodies(&reqs, "/v1/chat/completions");
    let b = &sent[0];
    assert_eq!(b["model"], digest::VISION_MODEL);
    assert_eq!(b["stream"], false);
    assert_eq!(b["max_tokens"], 2048);
    assert!(
        b.get("chat_template_kwargs").is_none(),
        "the instruct extraction keeps its request shape"
    );
    assert!(
        b.get("response_format").is_none(),
        "the extraction answers plain text"
    );
    let parts = b["messages"][0]["content"].as_array().unwrap();
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "image_url");
    assert!(
        parts[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    let page2: PageText =
        serde_json::from_slice(&std::fs::read(w.exec(EXEC).join("pages/2.json")).unwrap()).unwrap();
    assert_eq!(page2.text, "Invoice 42");
    assert!(!page2.truncated);
    // A second run calls nothing (the mock expects two calls in total).
    assert_eq!(
        digest::extract(w.cwd(), &params(EXEC, &[]), &client(&server))
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn extract_retries_a_page_once_then_fails() {
    let server = MockServer::start().await;
    let w = Work::new();
    rendered(&w, EXEC, 1, 0);
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(chat_answer("text", "length"), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;
    digest::extract(w.cwd(), &params(EXEC, &[]), &client(&server))
        .await
        .unwrap();
    let page: PageText =
        serde_json::from_slice(&std::fs::read(w.exec(EXEC).join("pages/1.json")).unwrap()).unwrap();
    assert!(
        page.truncated,
        "finish_reason length is stored as truncated"
    );

    let server = MockServer::start().await;
    let w = Work::new();
    rendered(&w, EXEC, 1, 0);
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .expect(2)
        .mount(&server)
        .await;
    let err = digest::extract(w.cwd(), &params(EXEC, &[]), &client(&server))
        .await
        .unwrap_err();
    assert!(err.contains("page 1"), "{err}");
    assert!(!w.exec(EXEC).join("pages/1.json").exists());
}

#[tokio::test]
async fn classify_stores_counts_without_span_text() {
    let server = MockServer::start().await;
    let w = Work::new();
    rendered(&w, EXEC, 2, 0);
    store_text(&w, EXEC, 1, "No names here.", false);
    store_text(&w, EXEC, 2, "Contact: John Smith, john@example.com", false);
    Mock::given(method("POST"))
        .and(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("classify")))
        .expect(2)
        .mount(&server)
        .await;
    let pii = digest::classify(w.cwd(), &params(EXEC, &[]), &client(&server))
        .await
        .unwrap();
    assert_eq!(pii.total["private_person"], 2);
    assert_eq!(pii.pages[1].counts["private_email"], 1);
    let stored = std::fs::read_to_string(w.exec(EXEC).join("pii.json")).unwrap();
    assert!(!stored.contains("John Smith"), "{stored}");
    assert!(!stored.contains("john@example.com"), "{stored}");
    let reqs = server.received_requests().await.unwrap();
    let sent = bodies(&reqs, "/v1/privacy/classify");
    assert_eq!(sent[0]["model"], "openai/privacy-filter");
    assert_eq!(sent[0]["input"], "No names here.");
}

#[tokio::test]
async fn a_model_outside_direct_models_is_refused_before_any_request() {
    let server = MockServer::start().await;
    let w = Work::new();
    rendered(&w, EXEC, 1, 0);
    store_text(&w, EXEC, 1, "x", false);
    let mut p = params(EXEC, &[]);
    p.insert(
        "direct_models".into(),
        serde_json::to_string(&[digest::DIGEST_MODEL]).unwrap(),
    );
    let err = digest::extract(w.cwd(), &p, &client(&server))
        .await
        .unwrap_err();
    assert!(err.contains(digest::VISION_MODEL), "{err}");
    let err = digest::classify(w.cwd(), &p, &client(&server))
        .await
        .unwrap_err();
    assert!(err.contains(digest::CLASSIFY_MODEL), "{err}");
    p.insert(
        "direct_models".into(),
        serde_json::to_string(&[digest::VISION_MODEL]).unwrap(),
    );
    std::fs::write(w.exec(EXEC).join("pii.json"), r#"{"pages":[],"total":{}}"#).unwrap();
    let err = digest::compose(w.cwd(), &p, &client(&server), &attestor(&server), &fast())
        .await
        .unwrap_err();
    assert!(err.contains(digest::DIGEST_MODEL), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// 4.3 compose
// ---------------------------------------------------------------------------

/// A completion responder that records the request bytes, and a signature
/// responder that signs them with the recorded report's signer.
#[derive(Clone)]
struct SignedServer {
    content: Arc<Mutex<String>>,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

struct Chat(SignedServer);
struct Sig(SignedServer);

/// The model a request names.
fn model_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body).unwrap()["model"]
        .as_str()
        .unwrap()
        .to_owned()
}

impl Respond for Chat {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.0.sent.lock().unwrap().push(req.body.clone());
        let content = self.0.content.lock().unwrap().clone();
        ResponseTemplate::new(200).set_body_raw(
            chat_answer_from(&model_of(&req.body), &content, "stop"),
            "application/json",
        )
    }
}

/// The classifier: a `private_person` span on every `Jane Doe` and a
/// `private_phone` span on every `+44 20 7946 0958`, offsets in
/// characters of the input.
struct Classifier;

pub const PERSON: &str = "Jane Doe";
pub const PHONE: &str = "+44 20 7946 0958";

fn spans_in(input: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (needle, category) in [(PERSON, "private_person"), (PHONE, "private_phone")] {
        for (byte, _) in input.match_indices(needle) {
            let start = input[..byte].chars().count();
            out.push(json!({"category": category, "start": start,
                "end": start + needle.chars().count(), "text": needle, "score": 0.99}));
        }
    }
    out
}

impl Respond for Classifier {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        let input = body["input"].as_str().unwrap();
        let mut answer = fixture("classify");
        answer["data"][0]["spans"] = Value::Array(spans_in(input));
        ResponseTemplate::new(200).set_body_json(answer)
    }
}

impl Respond for Sig {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let sent = self.0.sent.lock().unwrap();
        let request = sent.last().cloned().unwrap_or_default();
        let content = self.0.content.lock().unwrap().clone();
        let response = chat_answer_from(digest::DIGEST_MODEL, &content, "stop");
        let mut b = fixture("signature");
        b["text"] = format!(
            "{}:{}:{}",
            digest::DIGEST_MODEL,
            nearbee_core::journal::sha256_hex(&request),
            nearbee_core::journal::sha256_hex(&response)
        )
        .into();
        b["signature"] = format!("0x{}", "1b".repeat(65)).into();
        b["signing_address"] =
            fixture("report")["model_attestations"][0]["signing_address"].clone();
        ResponseTemplate::new(200).set_body_json(b)
    }
}

impl SignedServer {
    async fn mount(server: &MockServer, content: &str) -> Self {
        let s = Self {
            content: Arc::new(Mutex::new(content.to_owned())),
            sent: Arc::new(Mutex::new(Vec::new())),
        };
        let mut report = fixture("report");
        report["model_attestations"][0]["model_name"] = digest::DIGEST_MODEL.into();
        Mock::given(method("GET"))
            .and(path("/v1/attestation/report"))
            .respond_with(ResponseTemplate::new(200).set_body_json(report))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(Chat(s.clone()))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex("^/v1/signature/"))
            .respond_with(Sig(s.clone()))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/privacy/classify"))
            .respond_with(Classifier)
            .mount(server)
            .await;
        s
    }

    fn calls(&self) -> usize {
        self.sent.lock().unwrap().len()
    }

    fn last_request(&self) -> Value {
        serde_json::from_slice(self.sent.lock().unwrap().last().unwrap()).unwrap()
    }
}

fn answer_json() -> Value {
    json!({
        "summary": "An invoice from Acme Ltd for consulting.",
        "entities": [
            {"type": "organisation", "value": "Acme Ltd"},
            {"type": "amount", "value": "EUR 1,200.00"}
        ],
        "figures": [
            {"value": "1,200.00", "label": "Total", "page": 1},
            {"value": "42", "label": "Invoice number", "page": 1},
            {"value": "999", "label": "Wrong page", "page": 1},
            {"value": "1", "label": "Too short", "page": 2}
        ],
        "open_questions": ["Is the VAT included?"]
    })
}

/// An execution ready for compose: two extracted pages, page 2 cut, PII
/// counts stored.
fn ready(w: &Work, exec: &str) {
    rendered(w, exec, 2, 3);
    store_text(
        w,
        exec,
        1,
        "Invoice  42\nTotal: 1,200.00 EUR\nAcme Ltd",
        false,
    );
    store_text(w, exec, 2, "Terms: net 30. Reference 999 1", true);
    std::fs::write(
        w.exec(exec).join("pii.json"),
        r#"{"pages":[{"page":1,"counts":{}},{"page":2,"counts":{"private_person":2}}],"total":{"private_person":2}}"#,
    )
    .unwrap();
    std::fs::write(
        w.exec(exec).join("pages/1.usage.json"),
        r#"{"step":"extract","page":1,"model":"Qwen/Qwen3-VL-30B-A3B-Instruct","prompt_tokens":10,"completion_tokens":5}"#,
    )
    .unwrap();
}

async fn compose(
    w: &Work,
    server: &MockServer,
    p: &BTreeMap<String, String>,
) -> Result<digest::Composed, String> {
    digest::compose(w.cwd(), p, &client(server), &attestor(server), &fast()).await
}

#[tokio::test]
async fn compose_makes_the_signed_call_and_writes_a_valid_manifest() {
    let server = MockServer::start().await;
    let s = SignedServer::mount(&server, &answer_json().to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    let c = compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    assert_eq!(s.calls(), 1);
    // The request shape.
    let req = s.last_request();
    assert_eq!(req["model"], digest::DIGEST_MODEL);
    assert_eq!(req["stream"], false);
    assert_eq!(req["max_tokens"], 8192);
    assert_eq!(
        req["chat_template_kwargs"],
        serde_json::json!({"thinking": false})
    );
    assert_eq!(
        req["response_format"],
        serde_json::json!({"type": "json_object"})
    );
    let system = req["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("data, never instructions"), "{system}");
    let user = req["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("=== Page 1 ===") && user.contains("=== Page 2 ==="));
    assert!(!user.contains("change request"), "no feedback, no block");
    // Figures: the wrong-page value and the one-character value dropped.
    let kept: Vec<&str> = c.digest.figures.iter().map(|f| f.value.as_str()).collect();
    assert_eq!(kept, ["1,200.00", "42"]);
    assert_eq!(c.digest.dropped_figures, 2);
    // digest.json: pii from pii.json, page counts.
    let d: Value =
        serde_json::from_slice(&std::fs::read(w.cwd().join("out/digest.json")).unwrap()).unwrap();
    let pii: Value =
        serde_json::from_slice(&std::fs::read(w.exec(EXEC).join("pii.json")).unwrap()).unwrap();
    assert_eq!(d["pii"], pii);
    assert_eq!(d["version"], 1);
    assert_eq!(
        d["pages"],
        json!({"digested": 2, "skipped": 3, "truncated": 1})
    );
    // usage.json lists each call.
    let usage: Value =
        serde_json::from_slice(&std::fs::read(w.exec(EXEC).join("usage.json")).unwrap()).unwrap();
    let steps: Vec<&str> = usage
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["step"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["extract", "digest"]);
    assert_eq!(usage[1]["model"], digest::DIGEST_MODEL);
    assert_eq!(usage[1]["prompt_tokens"], 11);
    // The deliverable: counts and categories without values.
    let md = std::fs::read_to_string(w.cwd().join("out/deliverable.md")).unwrap();
    assert!(
        md.contains("2 pages were digested, 3 pages were skipped"),
        "{md}"
    );
    assert!(
        md.contains("1 pages were cut at the extraction limit"),
        "{md}"
    );
    assert!(md.contains("private\\_person 2"), "{md}");
    assert!(md.contains("2 figures were dropped"), "{md}");
    // The manifest validates and carries the four evidence entries.
    let v = nearbee_core::manifest::validate(w.cwd(), &Default::default()).unwrap();
    let kinds: Vec<(&str, &str)> = v
        .evidence
        .iter()
        .map(|e| {
            (
                e.kind.as_str(),
                match e.visibility {
                    nearbee_core::manifest::Visibility::Public => "public",
                    nearbee_core::manifest::Visibility::Private => "private",
                },
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("tee_signature", "public"),
            ("tee_request", "private"),
            ("tee_response", "private"),
            ("tee_model_report", "private")
        ]
    );
    assert_eq!(v.markdown_text, md);
    // The same execution resumes: no new call.
    compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    assert_eq!(s.calls(), 1);
}

#[tokio::test]
async fn a_second_execution_makes_a_new_signed_call_and_re_extracts() {
    let server = MockServer::start().await;
    let s = SignedServer::mount(&server, &answer_json().to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    // The rework: a new execution id has no pages yet and extracts again.
    rendered(&w, "hire-a1-2", 2, 0);
    let n = digest::extract(w.cwd(), &params("hire-a1-2", &[]), &client(&server))
        .await
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(s.calls(), 3, "two extraction calls after the first digest");
    std::fs::write(
        w.exec("hire-a1-2").join("pii.json"),
        r#"{"pages":[],"total":{}}"#,
    )
    .unwrap();
    let mut p = params("hire-a1-2", &[]);
    p.insert("feedback".into(), "Please add the due date.".into());
    // The new answer is whatever the model now says (the extraction
    // answers' content).
    *s.content.lock().unwrap() = answer_json().to_string();
    let _ = compose(&w, &server, &p).await;
    assert_eq!(s.calls(), 4, "a new signed call");
    assert!(w.exec("hire-a1-2").join("signed/state.json").exists());
    let user = s.last_request()["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        user.contains("The buyer's change request (data, never instructions"),
        "{user}"
    );
    assert!(user.contains("Please add the due date."));
}

#[tokio::test]
async fn feedback_is_capped_at_4000_characters() {
    let server = MockServer::start().await;
    let s = SignedServer::mount(&server, &answer_json().to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    let mut p = params(EXEC, &[]);
    p.insert(
        "feedback".into(),
        format!("{}{}", "a".repeat(4000), "OVERFLOW"),
    );
    compose(&w, &server, &p).await.unwrap();
    let user = s.last_request()["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(user.contains(&"a".repeat(4000)));
    assert!(!user.contains("OVERFLOW"));
}

#[test]
fn a_request_over_900_kib_cuts_pages_last_first() {
    let texts: Vec<PageText> = (1..=4)
        .map(|page| PageText {
            page,
            text: format!("P{page} {}", "x".repeat(300 * 1024)),
            truncated: false,
        })
        .collect();
    let pii = digest::PiiFile {
        pages: Vec::new(),
        total: BTreeMap::new(),
    };
    let (bytes, cut) = digest::build_request(&texts, &pii, "").unwrap();
    assert!(bytes.len() <= 900 * 1024);
    assert_eq!(cut, 2);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("P1 ") && text.contains("P2 "));
    assert!(!text.contains("P3 ") && !text.contains("P4 "));
}

#[tokio::test]
async fn an_answer_outside_the_schema_or_with_pii_leaves_no_manifest() {
    for (content, needle) in [
        ("not json".to_owned(), "not JSON"),
        (
            {
                let mut a = answer_json();
                a["pii"] = json!({"private_person": 1});
                a.to_string()
            },
            "pii",
        ),
        (
            {
                let mut a = answer_json();
                a["extra"] = json!(1);
                a.to_string()
            },
            "schema",
        ),
        (
            {
                let mut a = answer_json();
                a["summary"] = json!("");
                a.to_string()
            },
            "summary",
        ),
        (
            {
                let mut a = answer_json();
                a["entities"][0]["type"] = json!("person");
                a.to_string()
            },
            "entity",
        ),
        (
            {
                let mut a = answer_json();
                a["figures"][0]["page"] = json!(41);
                a.to_string()
            },
            "figure",
        ),
    ] {
        let server = MockServer::start().await;
        SignedServer::mount(&server, &content).await;
        let w = Work::new();
        ready(&w, EXEC);
        let err = compose(&w, &server, &params(EXEC, &[])).await.unwrap_err();
        assert!(err.contains(needle), "{needle}: {err}");
        assert!(!w.cwd().join("out/result.json").exists());
    }
}

#[tokio::test]
async fn a_fenced_json_answer_is_accepted() {
    let server = MockServer::start().await;
    SignedServer::mount(&server, &format!("```json\n{}\n```", answer_json())).await;
    let w = Work::new();
    ready(&w, EXEC);
    compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    assert!(w.cwd().join("out/result.json").exists());
}

#[test]
fn the_deliverable_escapes_links_images_fences_and_bare_addresses() {
    let d = digest::DigestFile {
        version: 1,
        summary: "See [click](https://example.com) and ![img](https://e.x/i.png)\n```\nfence\n``` https://x.y www.x.y a@b.c".into(),
        entities: vec![],
        figures: vec![],
        open_questions: vec!["<script>alert(1)</script>".into()],
        pii: digest::PiiFile {
            pages: vec![],
            total: BTreeMap::new(),
        },
        pages: digest::PageCounts {
            digested: 1,
            skipped: 0,
            truncated: 0,
        },
        dropped_figures: 0,
        dropped_personal_figures: 0,
        redacted_spans: 0,
    };
    let json = serde_json::to_string_pretty(&d).unwrap();
    let md = digest::render(&d, 0, &json);
    let prose = md.split("## Digest JSON").next().unwrap();
    // Only the escaped forms remain: no link, image, fence or bare
    // address renders.
    assert!(!prose.replace("\\](", "").contains("]("), "{prose}");
    assert!(!prose.replace("!\\[", "").contains("!["), "{prose}");
    assert!(!prose.replace("\\`", "").contains('`'), "{prose}");
    assert!(!prose.replace("\\[://\\]", "").contains("://"), "{prose}");
    assert!(!prose.replace("www\\[.\\]", "").contains("www."), "{prose}");
    assert!(!prose.replace("\\[@\\]", "").contains('@'), "{prose}");
    for escaped in [
        "https\\[://\\]x.y",
        "www\\[.\\]x.y",
        "a\\[@\\]b.c",
        "\\[click\\]",
    ] {
        assert!(prose.contains(escaped), "{escaped}: {prose}");
    }
    assert!(prose.contains("&lt;script&gt;"), "{prose}");
    assert!(prose.contains("The privacy classifier found no personal data."));
    // The JSON fence is longer than any backtick run inside it.
    assert!(md.contains("````json\n"), "{md}");
}

// ---------------------------------------------------------------------------
// Code review round: redaction, page counts, escaping, error texts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn compose_redacts_personal_data_in_every_free_text_value() {
    let server = MockServer::start().await;
    let mut a = answer_json();
    a["summary"] = format!("An invoice to {PERSON} from Acme Ltd.").into();
    a["entities"][0]["value"] = format!("{PERSON} Consulting").into();
    a["figures"] = json!([
        {"value": "1,200.00", "label": format!("Total owed by {PERSON}"), "page": 1},
        {"value": PHONE, "label": "Phone", "page": 1},
        {"value": "42", "label": "Invoice number", "page": 1}
    ]);
    a["open_questions"] = json!([format!("Is {PERSON} the payer?")]);
    SignedServer::mount(&server, &a.to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    // The phone number is on page 1, so the figure check keeps it.
    store_text(
        &w,
        EXEC,
        1,
        &format!("Invoice  42\nTotal: 1,200.00 EUR\nAcme Ltd\nPhone {PHONE}"),
        false,
    );
    let c = compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    assert_eq!(
        c.digest.summary,
        "An invoice to [redacted: private_person] from Acme Ltd."
    );
    assert_eq!(
        c.digest.entities[0].value,
        "[redacted: private_person] Consulting"
    );
    assert_eq!(
        c.digest.open_questions,
        ["Is [redacted: private_person] the payer?"]
    );
    let kept: Vec<(&str, &str)> = c
        .digest
        .figures
        .iter()
        .map(|f| (f.value.as_str(), f.label.as_str()))
        .collect();
    assert_eq!(
        kept,
        [
            ("1,200.00", "Total owed by [redacted: private_person]"),
            ("42", "Invoice number")
        ]
    );
    assert_eq!(c.digest.dropped_personal_figures, 1);
    assert_eq!(c.digest.redacted_spans, 4);
    let digest_json = std::fs::read_to_string(w.cwd().join("out/digest.json")).unwrap();
    let md = std::fs::read_to_string(w.cwd().join("out/deliverable.md")).unwrap();
    for text in [&digest_json, &md] {
        assert!(!text.contains(PERSON), "{text}");
        assert!(!text.contains(PHONE), "{text}");
    }
    assert!(
        md.contains("1 because their value holds personal data"),
        "{md}"
    );
    assert!(md.contains("4 spans of personal data"), "{md}");
    // The classifier saw the digest's own text, with no aliasing.
    let reqs = server.received_requests().await.unwrap();
    let classified: Vec<&Request> = reqs
        .iter()
        .filter(|r| r.url.path() == "/v1/privacy/classify")
        .collect();
    assert_eq!(classified.len(), 1, "one chunk");
    assert_eq!(classified[0].headers.get("x-no-aliasing").unwrap(), "true");
    let input = serde_json::from_slice::<Value>(&classified[0].body).unwrap()["input"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(input.starts_with("An invoice to Jane Doe"), "{input}");
}

#[tokio::test]
async fn long_digest_text_is_classified_in_chunks() {
    let server = MockServer::start().await;
    let mut a = answer_json();
    a["summary"] = format!("{} {PERSON}", "s".repeat(1990)).into();
    a["open_questions"] = (0..20)
        .map(|i| Value::from(format!("{i} {} {PERSON}", "q".repeat(480))))
        .collect::<Vec<_>>()
        .into();
    SignedServer::mount(&server, &a.to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    let c = compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    assert!(c.digest.summary.ends_with("[redacted: private_person]"));
    assert!(
        c.digest
            .open_questions
            .iter()
            .all(|q| q.ends_with("[redacted: private_person]"))
    );
    assert_eq!(c.digest.redacted_spans, 21);
    let reqs = server.received_requests().await.unwrap();
    let inputs: Vec<String> = bodies(&reqs, "/v1/privacy/classify")
        .iter()
        .map(|b| b["input"].as_str().unwrap().to_owned())
        .collect();
    assert!(inputs.len() > 1, "{}", inputs.len());
    assert!(
        inputs
            .iter()
            .all(|i| i.chars().count() <= digest::REDACT_CHUNK_CHARS)
    );
}

#[tokio::test]
async fn a_span_outside_the_digest_text_fails_closed() {
    let server = MockServer::start().await;
    let mut bad = fixture("classify");
    bad["data"][0]["spans"] = json!([{"category": "private_person", "start": 0, "end": 1_000_000}]);
    Mock::given(method("POST"))
        .and(path("/v1/privacy/classify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(bad))
        .with_priority(1)
        .mount(&server)
        .await;
    SignedServer::mount(&server, &answer_json().to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    let err = compose(&w, &server, &params(EXEC, &[])).await.unwrap_err();
    assert!(err.contains("outside the digest text"), "{err}");
    assert!(!w.cwd().join("out/result.json").exists());
    assert!(!w.cwd().join("out/digest.json").exists());
}

#[tokio::test]
async fn compose_without_the_classifier_in_direct_models_sends_nothing() {
    let server = MockServer::start().await;
    let w = Work::new();
    ready(&w, EXEC);
    let mut p = params(EXEC, &[]);
    p.insert(
        "direct_models".into(),
        serde_json::to_string(&[digest::DIGEST_MODEL]).unwrap(),
    );
    let err = compose(&w, &server, &p).await.unwrap_err();
    assert!(err.contains(digest::CLASSIFY_MODEL), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_page_cut_to_fit_the_request_counts_as_skipped_only() {
    let server = MockServer::start().await;
    SignedServer::mount(&server, &answer_json().to_string()).await;
    let w = Work::new();
    ready(&w, EXEC);
    // Page 2 is truncated and too large for the request: it is cut.
    store_text(&w, EXEC, 2, &"x".repeat(950 * 1024), true);
    let c = compose(&w, &server, &params(EXEC, &[])).await.unwrap();
    assert_eq!(c.cut_pages, 1);
    assert_eq!(
        c.digest.pages,
        digest::PageCounts {
            digested: 1,
            skipped: 4,
            truncated: 0
        }
    );
}

#[tokio::test]
async fn a_schema_error_names_the_field_and_never_the_content() {
    for (a, field) in [
        (
            {
                let mut a = answer_json();
                a["figures"][0]["page"] = "Invoice for Jane Doe".into();
                a
            },
            "nested field",
        ),
        (
            {
                let mut a = answer_json();
                a["summary"] = json!(["Invoice for Jane Doe"]);
                a
            },
            "summary",
        ),
        (
            {
                let mut a = answer_json();
                a["Jane Doe"] = json!(1);
                a
            },
            "unknown key",
        ),
    ] {
        let err = digest::parse_answer(&a.to_string()).unwrap_err();
        assert!(err.contains("schema"), "{err}");
        assert!(err.contains(field), "{field}: {err}");
        assert!(!err.contains("Jane"), "{err}");
    }
}

#[test]
fn setext_underlines_and_bare_domains_do_not_render() {
    let text = "Signature verified\n===\nPay at pay-here.example or Pay-Here.COM now\n  ==\n1,200.00 EUR, e.g. this";
    let out = digest::escape(text);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines[1], "\\===");
    assert_eq!(lines[3], "  \\==");
    assert!(lines[2].contains("pay-here\\[.\\]example"), "{out}");
    assert!(lines[2].contains("Pay-Here\\[.\\]COM"), "{out}");
    assert_eq!(lines[4], "1,200.00 EUR, e.g. this");
    // A `---` underline was already escaped as a bullet sign.
    assert_eq!(digest::escape("Heading\n---"), "Heading\n\\---");
}
