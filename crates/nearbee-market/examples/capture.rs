//! Records the market answers that need no hire (task 3.4) into replay
//! fixtures: `cargo run -p nearbee-market --example capture -- <handle>`.
//!
//! Design 3: the agent token comes from the credentials store by handle,
//! the origin is `MARKET_URL` (default `https://market.near.ai`), and every
//! recorded answer is scrubbed of tokens, capability-link segments and
//! presigned queries before it is written to
//! `crates/nearbee-market/tests/fixtures/market/<name>.json`.
//!
//! Captures (no hire needed):
//! - `agent`        the public agent record of the handle (404 for a draft
//!   agent: the id comes from the operator, see `agent add --agent-id`)
//! - `assignments`  `GET /v1/agents/me/assignments?status=all` (first page)
//! - `pricing-plans` the agent's pricing plans
//! - `file-trio`    the file reserve/PUT/complete trio with a delete of
//!   the unbound file
//!
//! The integrator runs this; nothing here is exercised by CI. The scrubber
//! is unit-tested offline (`src/scrub.rs`).

use std::io::Write as _;

use nearbee_core::credentials::Store;
use nearbee_core::secret::Secret;
use nearbee_market::scrub::{scrub_fixture, transcript_entry, transcript_json};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(handle) = args.next() else {
        eprintln!("usage: cargo run -p nearbee-market --example capture -- <handle>");
        std::process::exit(2);
    };
    let credentials = std::env::var("NEARBEE_CREDENTIALS")
        .ok()
        .map(std::path::PathBuf::from);
    let store = match credentials {
        Some(p) => Store::at(p),
        None => {
            let xdg = std::env::var("XDG_CONFIG_HOME").ok();
            let home = std::env::var("HOME").ok();
            match nearbee_core::credentials::credentials_path(xdg.as_deref(), home.as_deref()) {
                Some(p) => Store::at(p),
                None => {
                    eprintln!("error: cannot find a credentials directory (HOME is unset)");
                    std::process::exit(3);
                }
            }
        }
    };
    let creds = match store.get(&handle) {
        Ok(Some(c)) => c,
        Ok(None) => {
            eprintln!("error: no credentials stored for {handle}; run: nearbee agent add {handle}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(3);
        }
    };
    let base = std::env::var("MARKET_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://market.near.ai".into());
    let token = creds.token.clone();
    let mut redact = vec![token.clone()];
    if let Ok(k) = std::env::var("NEARBEE_REDACT") {
        redact.push(Secret::new(k));
    }
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("market");
    std::fs::create_dir_all(&out_dir).expect("fixtures dir");
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let failures = runtime.block_on(capture_all(&base, token, &redact, &out_dir, &handle));
    if failures.is_empty() {
        println!("captured into {}", out_dir.display());
    } else {
        for f in failures {
            eprintln!("error: {f}");
        }
        std::process::exit(1);
    }
}

/// Scrubs and writes one fixture file.
fn record(
    name: &str,
    body: &str,
    redact: &[Secret],
    out_dir: &std::path::Path,
) -> Result<(), String> {
    let path = out_dir.join(format!("{name}.json"));
    let scrubbed = scrub_fixture(body, redact);
    let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(scrubbed.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

async fn capture_all(
    base: &str,
    token: Secret,
    redact: &[Secret],
    out_dir: &std::path::Path,
    handle: &str,
) -> Vec<String> {
    let mut failures = Vec::new();
    let client = match reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => return vec![format!("http client: {e}")],
    };
    let agent_path = format!("/v1/agents/{handle}");
    let assignments_path = "/v1/agents/me/assignments?status=all&limit=100&offset=0";
    let plans_path = format!("/v1/agents/{handle}/pricing-plans");
    for (name, path) in [
        ("agent", agent_path.as_str()),
        ("assignments", assignments_path),
        ("pricing-plans", plans_path.as_str()),
    ] {
        match client
            .get(format!("{}{}", base.trim_end_matches('/'), path))
            .bearer_auth(token.expose())
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(resp) => match resp.text().await {
                Ok(text) => {
                    if let Err(e) = record(name, &text, redact, out_dir) {
                        failures.push(e);
                    }
                }
                Err(e) => failures.push(format!("{name}: {e}")),
            },
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }

    // The file trio with a delete of the unbound file. The bytes are
    // trivial and the file is never linked to a hire.
    let trio = capture_file_trio(&client, base, token.expose(), redact, out_dir).await;
    failures.extend(trio);
    failures
}

/// Reserve, PUT, complete and delete one unbound file, recording every
/// answer (and the final delete answer, whatever the route answers).
async fn capture_file_trio(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    redact: &[Secret],
    out_dir: &std::path::Path,
) -> Vec<String> {
    use serde_json::json;
    let origin = base.trim_end_matches('/');
    let mut transcript: Vec<String> = Vec::new();
    let reserve = client
        .post(format!("{origin}/v1/files"))
        .bearer_auth(token)
        .json(&json!({"name": "nearbee-capture.txt", "link": true}))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    let reserve = match reserve {
        Ok(r) => r,
        Err(e) => return vec![format!("file-trio reserve: {e}")],
    };
    let status = reserve.status();
    let body = reserve.text().await.unwrap_or_default();
    transcript.push(transcript_entry(
        "reserve",
        status.as_u16(),
        &serde_json::to_string(&body).unwrap_or_default(),
    ));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let upload_url = v
        .get("uploadUrl")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_owned();
    let file_id = v
        .get("fileId")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_owned();
    if upload_url.is_empty() || file_id.is_empty() {
        return vec!["file-trio: the reserve answer has no uploadUrl or fileId".into()];
    }
    match client
        .put(&upload_url)
        .body(b"nearbee capture\n".to_vec())
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => {
            let st = r.status();
            transcript.push(transcript_entry("put", st.as_u16(), ""));
        }
        Err(e) => return vec![format!("file-trio put: {e}")],
    }
    match client
        .post(format!("{origin}/v1/files/{file_id}/complete"))
        .bearer_auth(token)
        .json(&json!({}))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => {
            let st = r.status();
            let b = r.text().await.unwrap_or_default();
            transcript.push(transcript_entry(
                "complete",
                st.as_u16(),
                &serde_json::to_string(&b).unwrap_or_default(),
            ));
        }
        Err(e) => return vec![format!("file-trio complete: {e}")],
    }
    match client
        .delete(format!("{origin}/v1/files/{file_id}"))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => {
            let st = r.status();
            let b = r.text().await.unwrap_or_default();
            transcript.push(transcript_entry(
                "delete",
                st.as_u16(),
                &serde_json::to_string(&b).unwrap_or_default(),
            ));
        }
        Err(e) => return vec![format!("file-trio delete: {e}")],
    }
    match record("file-trio", &transcript_json(&transcript), redact, out_dir) {
        Ok(()) => Vec::new(),
        Err(e) => vec![e],
    }
}
