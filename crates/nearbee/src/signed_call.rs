//! `nearbee signed-call`: one non-streamed NEAR AI Cloud completion sent
//! from exact request bytes, kept with its exact response bytes and its
//! `provider_tee` signature (design 2 of the change
//! `doc-digest-and-paid-demo`, spec `signed-call`).
//!
//! The state lives in the out directory (`state.json`), so a retried
//! script node resumes and the completion is sent at most once per
//! directory: the API offers no idempotency key.

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use nearbee_cloud::{ChatAnswer, CloudClient, CloudError, SignatureAnswer};
use nearbee_core::catalog::Catalog;
use nearbee_core::evidence::{Attestor, new_nonce};
use nearbee_core::journal::sha256_hex;
use serde::{Deserialize, Serialize};

/// The largest request file sent, in bytes.
pub const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

/// The largest `max_tokens` a request may set.
pub const MAX_TOKENS: u64 = 16_384;

/// The state file in the out directory.
pub const STATE_FILE: &str = "state.json";

/// The evidence files the command writes in the out directory.
pub const REQUEST_FILE: &str = "request.bin";
pub const RESPONSE_FILE: &str = "response.bin";
pub const REPORT_FILE: &str = "report.json";
pub const SIGNATURE_FILE: &str = "signature.json";

/// The time bounds of one invocation (design 2.6).
#[derive(Debug, Clone)]
pub struct Timing {
    /// The bound of one invocation, checked before each phase.
    pub deadline: Duration,
    /// How long the signature fetch is retried on `NotYet`.
    pub signature_window: Duration,
    /// The wait between two signature fetches.
    pub signature_interval: Duration,
    /// The time kept back from the completion wait for the signature
    /// fetch, so a late answer can still be signed.
    pub signature_reserve: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            deadline: Duration::from_secs(360),
            signature_window: Duration::from_secs(60),
            signature_interval: Duration::from_secs(5),
            signature_reserve: Duration::from_secs(30),
        }
    }
}

/// The outcome of a signed call.
#[derive(Debug, Clone)]
pub struct Signed {
    pub completion_id: String,
    pub model: String,
    /// The first choice of the stored response.
    pub answer: ChatAnswer,
}

/// The phases of `state.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Preflight,
    Sending,
    Responded,
    Refused,
    Signed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    phase: Phase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl State {
    fn at(phase: Phase) -> Self {
        Self {
            phase,
            completion_id: None,
            status: None,
            reason: None,
        }
    }
}

/// The public evidence file (spec `signed-call`, Evidence entries).
pub use nearbee_core::signature::SignatureFile;

/// True for `[A-Za-z0-9_-]{1,128}` (completion and execution ids).
pub use nearbee_core::signature::is_safe_id;

fn is_hex_prefixed(s: &str, digits: usize) -> bool {
    s.len() == digits + 2 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Writes `bytes` to `dir/name` through a temporary file, synced, renamed
/// and with the directory synced, so the file's presence means it is
/// complete.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let target = dir.join(name);
    let fail = |e: std::io::Error| format!("{}: {e}", target.display());
    match std::fs::symlink_metadata(&tmp) {
        Ok(_) => std::fs::remove_file(&tmp).map_err(fail)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(fail(e)),
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(fail)?;
    f.write_all(bytes).map_err(fail)?;
    f.sync_all().map_err(fail)?;
    drop(f);
    std::fs::rename(&tmp, &target).map_err(fail)?;
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(fail)
}

/// Reads a regular file of at most `cap` bytes (no symlink).
pub fn read_capped(path: &Path, cap: u64, cap_text: &str) -> Result<Vec<u8>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if meta.len() > cap {
        return Err(format!("{} exceeds {cap_text}", path.display()));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(cap + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > cap {
        return Err(format!("{} exceeds {cap_text}", path.display()));
    }
    Ok(bytes)
}

fn read_state(dir: &Path) -> Result<Option<State>, String> {
    let path = dir.join(STATE_FILE);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
        Ok(_) => {}
    }
    let bytes = read_capped(&path, 64 * 1024, "64 KiB")?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("{} does not parse: {e}", path.display()))
}

fn write_state(dir: &Path, state: &State) -> Result<(), String> {
    let bytes = serde_json::to_vec(state).map_err(|e| e.to_string())?;
    write_atomic(dir, STATE_FILE, &bytes)
}

/// Checks the request bytes (design 2.1) and returns its model.
fn check_request(bytes: &[u8], allowed: &[String]) -> Result<String, String> {
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err("the request exceeds 1 MiB".into());
    }
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "the request is not JSON".to_owned())?;
    let Some(obj) = v.as_object() else {
        return Err("the request is not a JSON object".into());
    };
    if obj.get("stream") != Some(&serde_json::Value::Bool(false)) {
        return Err("the request must set \"stream\": false".into());
    }
    match obj.get("max_tokens").and_then(serde_json::Value::as_u64) {
        None => return Err("the request must set max_tokens".into()),
        Some(n) if n > MAX_TOKENS => {
            return Err(format!("max_tokens {n} is above {MAX_TOKENS}"));
        }
        Some(_) => {}
    }
    let Some(model) = obj.get("model").and_then(serde_json::Value::as_str) else {
        return Err("the request has no model".into());
    };
    if !allowed.iter().any(|m| m == model) {
        return Err(format!(
            "the model {model} is not in the hire's direct models"
        ));
    }
    let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
    if model.starts_with("nearai/") || !catalog.is_nearai(model) {
        return Err(format!(
            "the model {model} is not a catalog model owned by nearai"
        ));
    }
    Ok(model.to_owned())
}

/// The signing addresses of a report's model attestations.
fn report_signers(report: &[u8]) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(report).map_err(|_| "the model report is not JSON".to_owned())?;
    let list = v
        .get("model_attestations")
        .and_then(serde_json::Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| {
            "the model report has no model_attestations; no completion is sent".to_owned()
        })?;
    let signers: Vec<String> = list
        .iter()
        .filter_map(|a| a.get("signing_address").and_then(serde_json::Value::as_str))
        .filter(|a| !a.trim().is_empty())
        .map(str::to_owned)
        .collect();
    if signers.is_empty() {
        return Err(
            "no model attestation of the model report carries a signing_address; no completion \
             is sent"
                .into(),
        );
    }
    Ok(signers)
}

fn check_deadline(started: Instant, timing: &Timing) -> Result<(), String> {
    if started.elapsed() >= timing.deadline {
        Err(bound_reached(timing))
    } else {
        Ok(())
    }
}

fn bound_reached(timing: &Timing) -> String {
    format!(
        "the invocation reached its {} s bound",
        timing.deadline.as_secs()
    )
}

fn refused_text(state: &State) -> String {
    match (state.status, state.reason.as_deref()) {
        (Some(s), _) => format!("the completion was refused with status {s}; not resent"),
        (None, Some("compressed")) => {
            "the completion answer was compressed; refused, not resent".into()
        }
        (None, Some("truncated")) => "the answer was cut at max_tokens; refused, not resent".into(),
        (None, Some("too_large")) => {
            "the completion answer exceeds 4 MiB; refused, not resent".into()
        }
        (None, Some("invalid_id")) => {
            "the completion id is not [A-Za-z0-9_-]{1,128}; refused, not resent".into()
        }
        (None, Some(r)) => format!("the completion was refused ({r}); not resent"),
        (None, None) => "the completion was refused; not resent".into(),
    }
}

fn refuse(dir: &Path, status: Option<u16>, reason: Option<&str>) -> String {
    let state = State {
        phase: Phase::Refused,
        completion_id: None,
        status,
        reason: reason.map(str::to_owned),
    };
    let text = refused_text(&state);
    match write_state(dir, &state) {
        Ok(()) => text,
        Err(e) => format!("{text} (the state could not be written: {e})"),
    }
}

/// Parses a stored response: refuses a cut answer and an unsafe id.
/// Returns the completion id and the first choice.
fn parse_response(dir: &Path, bytes: &[u8]) -> Result<(String, ChatAnswer), String> {
    let answer = nearbee_cloud::parse_chat(bytes)
        .map_err(|e| format!("the completion answer does not parse: {e}"))?;
    if answer.finish_reason != "stop" {
        return Err(refuse(dir, None, Some("truncated")));
    }
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let id = v
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !is_safe_id(id) {
        return Err(refuse(dir, None, Some("invalid_id")));
    }
    Ok((id.to_owned(), answer))
}

fn read_file(dir: &Path, name: &str, cap: u64) -> Result<Vec<u8>, String> {
    read_capped(&dir.join(name), cap, "its bound")
}

/// The model of the stored request (already checked when it was written).
fn stored_model(request: &[u8], allowed: &[String]) -> Result<String, String> {
    check_request(request, allowed)
}

/// Runs the signed call with its state in `out_dir` (design 2.3).
pub async fn signed_call(
    client: &CloudClient,
    attestor: &impl Attestor,
    request_path: &Path,
    out_dir: &Path,
    allowed_models: &[String],
    timing: &Timing,
) -> Result<Signed, String> {
    let started = Instant::now();
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let state = read_state(out_dir)?;
    let phase = state.as_ref().map(|s| s.phase);
    match phase {
        Some(Phase::Refused) => {
            return Err(refused_text(state.as_ref().expect("phase read")));
        }
        Some(Phase::Signed) => return finished(out_dir),
        Some(Phase::Sending) if !out_dir.join(RESPONSE_FILE).exists() => {
            return Err("the completion outcome is unknown; not resent".into());
        }
        _ => {}
    }

    let (request, model) = match phase {
        None => {
            let bytes = read_capped(request_path, MAX_REQUEST_BYTES, "1 MiB")?;
            let model = check_request(&bytes, allowed_models)?;
            write_atomic(out_dir, REQUEST_FILE, &bytes)?;
            check_deadline(started, timing)?;
            let nonce = new_nonce()?;
            let report = attestor.report(&model, &nonce).await?;
            report_signers(&report)?;
            write_atomic(out_dir, REPORT_FILE, &report)?;
            write_state(out_dir, &State::at(Phase::Preflight))?;
            (bytes, model)
        }
        Some(_) => {
            let bytes = read_file(out_dir, REQUEST_FILE, MAX_REQUEST_BYTES)?;
            let model = stored_model(&bytes, allowed_models)?;
            (bytes, model)
        }
    };

    let completion_id = match phase {
        None | Some(Phase::Preflight) => {
            check_deadline(started, timing)?;
            let limit = timing
                .deadline
                .saturating_sub(started.elapsed())
                .saturating_sub(timing.signature_reserve);
            if limit.is_zero() {
                // Nothing left to wait with: fail while still `preflight`.
                return Err(bound_reached(timing));
            }
            write_state(out_dir, &State::at(Phase::Sending))?;
            let sent = Instant::now();
            let raw = match client.chat_exact(&request, limit).await {
                Ok(raw) => raw,
                Err(CloudError::TooLarge(_)) => {
                    return Err(refuse(out_dir, None, Some("too_large")));
                }
                // No answer: the outcome is unknown and `sending` stays.
                // The client's wait ends only once `limit` has passed.
                Err(_) if sent.elapsed() >= limit => {
                    return Err(format!(
                        "the completion did not answer within the invocation's {} s bound; not resent",
                        timing.deadline.as_secs()
                    ));
                }
                Err(e) => return Err(format!("the completion failed: {e}; not resent")),
            };
            write_atomic(out_dir, RESPONSE_FILE, &raw.bytes)?;
            if !(200..300).contains(&raw.status) {
                return Err(refuse(out_dir, Some(raw.status), None));
            }
            if raw
                .content_encoding
                .as_deref()
                .is_some_and(|c| !c.trim().eq_ignore_ascii_case("identity"))
            {
                return Err(refuse(out_dir, None, Some("compressed")));
            }
            responded(out_dir, &raw.bytes)?
        }
        Some(Phase::Sending) => {
            let bytes = read_file(
                out_dir,
                RESPONSE_FILE,
                nearbee_cloud::MAX_RESPONSE_BYTES as u64,
            )?;
            responded(out_dir, &bytes)?
        }
        Some(Phase::Responded) => state
            .and_then(|s| s.completion_id)
            .filter(|id| is_safe_id(id))
            .ok_or_else(|| format!("{STATE_FILE} has no usable completion id"))?,
        Some(Phase::Refused | Phase::Signed) => unreachable!("handled above"),
    };

    let response = read_file(
        out_dir,
        RESPONSE_FILE,
        nearbee_cloud::MAX_RESPONSE_BYTES as u64,
    )?;
    let report = read_file(out_dir, REPORT_FILE, nearbee_cloud::MAX_BODY_BYTES as u64)?;
    let signers = report_signers(&report)?;
    let sig = fetch_signature(client, &completion_id, &model, started, timing).await?;
    let file = check_signature(&sig, &completion_id, &model, &request, &response, &signers)?;
    let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    write_atomic(out_dir, SIGNATURE_FILE, &bytes)?;
    write_state(
        out_dir,
        &State {
            completion_id: Some(completion_id),
            ..State::at(Phase::Signed)
        },
    )?;
    finished(out_dir)
}

/// Parses a stored response and records `responded`.
fn responded(dir: &Path, bytes: &[u8]) -> Result<String, String> {
    let (id, _) = parse_response(dir, bytes)?;
    write_state(
        dir,
        &State {
            completion_id: Some(id.clone()),
            ..State::at(Phase::Responded)
        },
    )?;
    Ok(id)
}

/// The outcome of a `signed` directory, read from its files.
fn finished(dir: &Path) -> Result<Signed, String> {
    let sig: SignatureFile = serde_json::from_slice(&read_file(dir, SIGNATURE_FILE, 64 * 1024)?)
        .map_err(|e| format!("{SIGNATURE_FILE} does not parse: {e}"))?;
    let response = read_file(dir, RESPONSE_FILE, nearbee_cloud::MAX_RESPONSE_BYTES as u64)?;
    let answer = nearbee_cloud::parse_chat(&response)
        .map_err(|e| format!("the stored answer does not parse: {e}"))?;
    Ok(Signed {
        completion_id: sig.completion_id,
        model: sig.model,
        answer,
    })
}

async fn fetch_signature(
    client: &CloudClient,
    id: &str,
    model: &str,
    started: Instant,
    timing: &Timing,
) -> Result<SignatureAnswer, String> {
    check_deadline(started, timing)?;
    let window_end = Instant::now() + timing.signature_window;
    let deadline = started + timing.deadline;
    let end = window_end.min(deadline);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let attempt = tokio::time::timeout(left, client.signature(id, model)).await;
        match attempt {
            Ok(Ok(sig)) => return Ok(sig),
            Ok(Err(CloudError::NotYet)) | Err(_) => {}
            Ok(Err(e)) => return Err(format!("the signature fetch failed: {e}")),
        }
        let now = Instant::now();
        if now + timing.signature_interval >= end {
            return Err(
                "the signature is not available within its window; the completion is kept and \
                 the step can be retried"
                    .into(),
            );
        }
        tokio::time::sleep(timing.signature_interval).await;
    }
}

/// The checks of spec `signed-call`, Signature.
fn check_signature(
    sig: &SignatureAnswer,
    completion_id: &str,
    model: &str,
    request: &[u8],
    response: &[u8],
    signers: &[String],
) -> Result<SignatureFile, String> {
    if sig.signature_kind.as_deref() != Some("provider_tee") {
        return Err(
            "the signature is not provider_tee: the response is not bound to a model TEE".into(),
        );
    }
    if sig.signing_algo != "ecdsa" {
        return Err("the signature is not ecdsa".into());
    }
    let request_sha256 = sha256_hex(request);
    let response_sha256 = sha256_hex(response);
    let expected = format!("{model}:{request_sha256}:{response_sha256}");
    if sig.text != expected {
        return Err("the signed text does not match the request and response bytes".into());
    }
    if !is_hex_prefixed(&sig.signing_address, 40) || !is_hex_prefixed(&sig.signature, 130) {
        return Err("the signature fields are malformed".into());
    }
    // Instances of one model share one signing key, so several
    // attestations may carry the same address: one match is enough.
    if !signers
        .iter()
        .any(|a| a.eq_ignore_ascii_case(&sig.signing_address))
    {
        return Err(
            "the signing address matches no model attestation of the preflight report".into(),
        );
    }
    Ok(SignatureFile {
        version: 1,
        completion_id: completion_id.to_owned(),
        model: model.to_owned(),
        signature_kind: "provider_tee".into(),
        signing_algo: sig.signing_algo.clone(),
        signing_address: sig.signing_address.clone(),
        signed_text: sig.text.clone(),
        signature: sig.signature.clone(),
        request_sha256,
        response_sha256,
    })
}

/// Parses the `direct_models` run param (compact JSON list of ids).
pub fn parse_direct_models(
    params: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let raw = params
        .get("direct_models")
        .ok_or_else(|| "the run param direct_models is missing".to_owned())?;
    serde_json::from_str::<Vec<String>>(raw)
        .map_err(|_| "the run param direct_models is not a JSON list of model ids".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_ids() {
        assert!(is_safe_id("chatcmpl-1_A"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id(&"a".repeat(129)));
        assert!(is_safe_id(&"a".repeat(128)));
    }

    #[test]
    fn hex_fields() {
        assert!(is_hex_prefixed(&format!("0x{}", "aB".repeat(20)), 40));
        assert!(!is_hex_prefixed(&"ab".repeat(21), 40));
        assert!(!is_hex_prefixed(&format!("0x{}", "zz".repeat(20)), 40));
    }
}
