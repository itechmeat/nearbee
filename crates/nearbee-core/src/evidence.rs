//! Endpoint attestation evidence of attested hires (design 6 of the change
//! `repo-audit-and-tiers`, spec `run-evidence`).
//!
//! The core sees NEAR AI Cloud only through [`Attestor`]: one report per
//! model and nonce, as raw bytes. This module also holds the private
//! snapshot record a fetched report is stored as, the nonce, and the
//! Markdown disclosure an attested deliverable carries. Reports are
//! collected and disclosed, never verified.

use std::future::Future;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::journal;
use crate::runner::{AttemptModel, Reported};

/// The bound of one whole report fetch, both attempts included (spec
/// `run-evidence`, The report request). Serve does not start a fetch when
/// the round's cut-off is nearer than this.
pub const FETCH_BOUND: std::time::Duration = std::time::Duration::from_secs(20);

/// The failed fetches of one phase in one generation after which the hire
/// is declined (spec `run-evidence`, Fail closed).
pub const MAX_FAILURES: usize = 3;

/// The longest failure reason a journal record keeps, in characters.
pub const REASON_MAX_CHARS: usize = 200;

/// The base of the public Verification Center link per model.
pub const VERIFY_URL: &str = "https://cloud.near.ai/verify?model=";

/// The first disclosure sentence (spec `run-evidence`, Disclosure
/// wording).
pub const NOT_VERIFIED: &str = "Nearbee collected these endpoint attestation reports from NEAR AI Cloud and did not verify them.";

/// The second disclosure sentence.
pub const NOT_BOUND: &str = "The reports show which endpoints attest as TEEs; they do not bind this job's responses to a TEE.";

/// The source of endpoint attestation reports (the seam behind which
/// `nearbee-cloud` calls NEAR AI Cloud). `report` returns the raw body of
/// a successful report or a short failure reason that never contains a
/// credential.
pub trait Attestor: Send + Sync {
    fn report(
        &self,
        model: &str,
        nonce: &str,
    ) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
}

/// An attestor that cannot fetch: every call fails with its reason and
/// sends nothing (no key configured, or no attested listing served).
#[derive(Debug, Clone)]
pub struct Unavailable(pub String);

impl Attestor for Unavailable {
    async fn report(&self, _model: &str, _nonce: &str) -> Result<Vec<u8>, String> {
        Err(self.0.clone())
    }
}

/// When a report is collected: before the generation's market start and
/// apb start, or after its run succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Start,
    End,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Start => "start",
            Phase::End => "end",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "start" => Some(Phase::Start),
            "end" => Some(Phase::End),
            _ => None,
        }
    }
}

/// The flat snapshot name of one report of a generation (design 6.3):
/// `evidence-<phase>-<first 16 hex of sha256(model)>.json`.
pub fn snapshot_name(phase: Phase, model: &str) -> String {
    let digest = journal::sha256_hex(model.as_bytes());
    format!("evidence-{}-{}.json", phase.as_str(), &digest[..16])
}

/// A fresh random 32-byte nonce as 64 lowercase hex digits.
pub fn new_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random nonce: {e}"))?;
    Ok(hex::encode(bytes))
}

/// A failure reason as the journal keeps it: at most
/// [`REASON_MAX_CHARS`] characters, bearer tokens removed.
pub fn short_reason(reason: &str) -> String {
    let redacted = crate::redact::redact_detail(reason, &[]);
    redacted.chars().take(REASON_MAX_CHARS).collect()
}

/// One stored report (design 6.3): the private snapshot file's content.
/// `sha256` is the digest of the decoded body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSnapshot {
    pub model: String,
    pub phase: String,
    pub nonce: String,
    pub fetched_at: String,
    pub sha256: String,
    pub body_b64: String,
}

impl EvidenceSnapshot {
    /// The record of a report fetched at `fetched_at`.
    pub fn new(
        model: &str,
        phase: Phase,
        nonce: &str,
        fetched_at: OffsetDateTime,
        body: &[u8],
    ) -> Self {
        Self {
            model: model.to_owned(),
            phase: phase.as_str().to_owned(),
            nonce: nonce.to_owned(),
            fetched_at: fetched_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            sha256: journal::sha256_hex(body),
            body_b64: base64::engine::general_purpose::STANDARD.encode(body),
        }
    }

    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an evidence snapshot serializes")
    }

    /// Reads a stored report and checks it: it parses, names `phase` and
    /// `model`, and its decoded body hashes to its recorded `sha256`.
    /// Anything else is an error, never a report to reuse.
    pub fn read_checked(bytes: &[u8], phase: Phase, model: &str) -> Result<Self, String> {
        let s: Self = serde_json::from_slice(bytes)
            .map_err(|e| format!("the evidence snapshot does not parse: {e}"))?;
        if s.phase != phase.as_str() || s.model != model {
            return Err("the evidence snapshot names another phase or model".to_owned());
        }
        let body = base64::engine::general_purpose::STANDARD
            .decode(s.body_b64.as_bytes())
            .map_err(|e| format!("the evidence snapshot body does not decode: {e}"))?;
        if journal::sha256_hex(&body) != s.sha256 {
            return Err("the evidence snapshot body does not match its hash".to_owned());
        }
        Ok(s)
    }
}

/// One collected report as the disclosure names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    pub model: String,
    pub sha256: String,
    pub fetched_at: String,
}

impl From<&EvidenceSnapshot> for Collected {
    fn from(s: &EvidenceSnapshot) -> Self {
        Self {
            model: s.model.clone(),
            sha256: s.sha256.clone(),
            fetched_at: s.fetched_at.clone(),
        }
    }
}

/// The Verification Center link of a model, its id URL-encoded (`/` as
/// `%2F`).
pub fn verify_link(model: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(model.as_bytes()).collect();
    format!("{VERIFY_URL}{encoded}")
}

/// Text inside a Markdown code span: a backtick cannot close it early.
fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "'"))
}

/// The `Endpoint attestation disclosure` section of spec `run-evidence`,
/// Disclosure: per model of the frozen set its id, both report hashes and
/// fetch times and the Verification Center link; per node attempt of the
/// delivered run the model apb requested (or `not recorded`) and the
/// reported model, `none reported` or `mixed`; and both wording
/// sentences.
pub fn section(
    models: &[String],
    start: &[Collected],
    end: &[Collected],
    attempts: &[AttemptModel],
) -> String {
    let mut out = String::from("## Endpoint attestation disclosure\n\n");
    out.push_str(NOT_VERIFIED);
    out.push(' ');
    out.push_str(NOT_BOUND);
    out.push_str("\n\n### Models\n");
    for model in models {
        out.push_str(&format!("\n- {}\n", code(model)));
        for (label, set) in [("start", start), ("end", end)] {
            match set.iter().find(|c| &c.model == model) {
                Some(c) => out.push_str(&format!(
                    "  - {label} report: SHA-256 {}, fetched {}\n",
                    code(&c.sha256),
                    c.fetched_at
                )),
                None => out.push_str(&format!("  - {label} report: missing\n")),
            }
        }
        out.push_str(&format!("  - verify: {}\n", verify_link(model)));
    }
    out.push_str("\n### Node attempts\n\n");
    if attempts.is_empty() {
        out.push_str("- no attempts were recorded\n");
    }
    for a in attempts {
        let requested = a
            .requested_model
            .as_deref()
            .map_or_else(|| "not recorded".to_owned(), code);
        let reported = match &a.reported {
            Reported::None => "none reported".to_owned(),
            Reported::One(m) => code(m),
            Reported::Mixed => "mixed".to_owned(),
        };
        out.push_str(&format!(
            "- node {}, attempt {}: requested {requested}, reported {reported}\n",
            code(&a.node),
            a.attempt
        ));
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";
    const QWEN: &str = "Qwen/Qwen3.8-27B";

    fn collected(model: &str, hash: &str, at: &str) -> Collected {
        Collected {
            model: model.into(),
            sha256: hash.into(),
            fetched_at: at.into(),
        }
    }

    fn attempt(node: &str, n: u32, requested: Option<&str>, reported: Reported) -> AttemptModel {
        AttemptModel {
            node: node.into(),
            attempt: n,
            agent: "opencode".into(),
            requested_model: requested.map(str::to_owned),
            reported,
        }
    }

    #[test]
    fn the_section_names_every_model_with_both_reports_and_its_link() {
        let models = vec![DS.to_owned(), QWEN.to_owned()];
        let start = vec![
            collected(DS, "aa11", "2026-11-11T10:00:00Z"),
            collected(QWEN, "bb22", "2026-11-11T10:00:45Z"),
        ];
        let end = vec![
            collected(DS, "cc33", "2026-11-11T11:00:00Z"),
            collected(QWEN, "dd44", "2026-11-11T11:00:45Z"),
        ];
        let s = section(&models, &start, &end, &[]);
        assert!(s.starts_with("## Endpoint attestation disclosure\n"), "{s}");
        for (model, h1, h2) in [(DS, "aa11", "cc33"), (QWEN, "bb22", "dd44")] {
            assert!(s.contains(&format!("- `{model}`")), "{s}");
            assert!(s.contains(&format!("start report: SHA-256 `{h1}`")), "{s}");
            assert!(s.contains(&format!("end report: SHA-256 `{h2}`")), "{s}");
        }
        assert!(s.contains("fetched 2026-11-11T10:00:00Z"));
        assert!(s.contains("fetched 2026-11-11T11:00:45Z"));
        assert!(
            s.contains("https://cloud.near.ai/verify?model=deepseek-ai%2FDeepSeek-V4.1-Flash-1017"),
            "{s}"
        );
        assert!(s.contains("https://cloud.near.ai/verify?model=Qwen%2FQwen3.8-27B"));
        // Models keep the frozen set's order.
        assert!(s.find(DS).unwrap() < s.find(QWEN).unwrap());
    }

    #[test]
    fn the_section_lines_up_every_attempt_without_substituting_the_profile_model() {
        let attempts = vec![
            attempt("audit", 1, Some(&format!("nearai/{DS}")), Reported::None),
            attempt("audit", 2, None, Reported::One(format!("nearai/{QWEN}"))),
            attempt("audit", 3, Some(QWEN), Reported::Mixed),
        ];
        let s = section(&[DS.to_owned()], &[], &[], &attempts);
        assert!(
            s.contains(&format!(
                "- node `audit`, attempt 1: requested `nearai/{DS}`, reported none reported"
            )),
            "{s}"
        );
        assert!(
            s.contains(&format!(
                "- node `audit`, attempt 2: requested not recorded, reported `nearai/{QWEN}`"
            )),
            "{s}"
        );
        assert!(
            s.contains(&format!(
                "- node `audit`, attempt 3: requested `{QWEN}`, reported mixed"
            )),
            "{s}"
        );
        // A missing report is named, never invented.
        assert!(s.contains("start report: missing"));
        assert!(s.contains("end report: missing"));
    }

    #[test]
    fn the_section_carries_both_wording_sentences() {
        let s = section(&[QWEN.to_owned()], &[], &[], &[]);
        assert!(s.contains(NOT_VERIFIED), "{s}");
        assert!(s.contains(NOT_BOUND), "{s}");
        assert!(s.contains("- no attempts were recorded"));
        assert!(!s.ends_with('\n'));
    }

    #[test]
    fn a_snapshot_round_trips_and_its_hash_is_checked_on_reuse() {
        let at = time::macros::datetime!(2026-11-11 10:00:00 UTC);
        let body = br#"{"gateway_attestation":{}}"#;
        let s = EvidenceSnapshot::new(QWEN, Phase::Start, "ab".repeat(32).as_str(), at, body);
        assert_eq!(s.sha256, journal::sha256_hex(body));
        assert_eq!(s.fetched_at, "2026-11-11T10:00:00Z");
        let bytes = s.to_bytes();
        let back = EvidenceSnapshot::read_checked(&bytes, Phase::Start, QWEN).unwrap();
        assert_eq!(back, s);
        // Another phase or model is not a reuse.
        assert!(EvidenceSnapshot::read_checked(&bytes, Phase::End, QWEN).is_err());
        assert!(EvidenceSnapshot::read_checked(&bytes, Phase::Start, DS).is_err());
        // A body that does not match its hash is refused.
        let mut tampered = s.clone();
        tampered.body_b64 = base64::engine::general_purpose::STANDARD.encode(b"{}");
        assert!(EvidenceSnapshot::read_checked(&tampered.to_bytes(), Phase::Start, QWEN).is_err());
        assert!(EvidenceSnapshot::read_checked(b"not json", Phase::Start, QWEN).is_err());
    }

    #[test]
    fn snapshot_names_are_flat_and_differ_per_phase_and_model() {
        let a = snapshot_name(Phase::Start, QWEN);
        let b = snapshot_name(Phase::End, QWEN);
        let c = snapshot_name(Phase::Start, DS);
        assert!(
            a.starts_with("evidence-start-") && a.ends_with(".json"),
            "{a}"
        );
        assert_eq!(a.len(), "evidence-start-".len() + 16 + ".json".len());
        assert!(b.starts_with("evidence-end-"));
        assert_ne!(a, c);
        assert!(!a.contains('/'));
    }

    #[test]
    fn a_nonce_is_64_hex_and_fresh() {
        let a = new_nonce().unwrap();
        let b = new_nonce().unwrap();
        assert_eq!(a.len(), 64);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_short_reason_is_bounded_and_drops_bearer_tokens() {
        let r = short_reason(&format!("Authorization: Bearer sk-123 {}", "x".repeat(500)));
        assert!(!r.contains("sk-123"), "{r}");
        assert_eq!(r.chars().count(), REASON_MAX_CHARS);
    }

    #[tokio::test]
    async fn the_unavailable_attestor_fails_with_its_reason() {
        let a = Unavailable("NEARAI_API_KEY is not set".into());
        assert_eq!(
            a.report(QWEN, "00").await.unwrap_err(),
            "NEARAI_API_KEY is not set"
        );
    }
}
