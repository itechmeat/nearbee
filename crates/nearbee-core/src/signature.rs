//! The response signature recheck at delivery (design 6 of the change
//! `doc-digest-and-paid-demo`, spec `hire-serving`, Response signature
//! recheck, Signature field formats, Required signature, Response
//! signature section).
//!
//! A pure function over the validated manifest: the bytes it checks are
//! the ones the manifest validation returned, never read by path again.
//! Signatures are not verified cryptographically here: the buyer's
//! Verification Center does that; serve checks that the signed text binds
//! exactly the stored request and response bytes and that every field is
//! well formed before any of it reaches Markdown or a URL.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::journal::sha256_hex;
use crate::manifest::{ValidatedEvidence, ValidatedManifest, Visibility};

/// The evidence kinds of a signed call.
pub const KIND_SIGNATURE: &str = "tee_signature";
pub const KIND_REQUEST: &str = "tee_request";
pub const KIND_RESPONSE: &str = "tee_response";

/// The attention codes of a failed recheck.
pub const SIGNATURE_MISMATCH: &str = "signature_mismatch";
pub const SIGNATURE_MISSING: &str = "signature_missing";

/// The Verification Center page a receipt link opens.
pub const VERIFY_URL: &str = "https://cloud.near.ai/verify";

/// The public evidence file `signature.json` of a signed call (spec
/// `signed-call`, Evidence entries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureFile {
    pub version: u32,
    pub completion_id: String,
    pub model: String,
    pub signature_kind: String,
    pub signing_algo: String,
    pub signing_address: String,
    pub signed_text: String,
    pub signature: String,
    pub request_sha256: String,
    pub response_sha256: String,
}

/// The receipt the Verification Center reads, keys in this order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt<'a> {
    completion_id: &'a str,
    model: &'a str,
    signature_kind: &'a str,
    signing_algo: &'a str,
    signing_address: &'a str,
    signed_text: &'a str,
    signature: &'a str,
}

/// A rechecked signature: the Markdown section and the two private files
/// the delivery snapshots when it opens the submit intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedSection {
    pub markdown: String,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
}

/// Why the recheck refused the deliverable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Mismatch {
    /// An entry is malformed or does not bind the stored bytes.
    #[error("the response signature does not check: {0}")]
    Mismatch(String),
    /// The listing signs a model call and the run has no valid entry.
    #[error("the response signature is missing: {0}")]
    Missing(String),
}

impl Mismatch {
    /// The attention code.
    pub fn code(&self) -> &'static str {
        match self {
            Mismatch::Mismatch(_) => SIGNATURE_MISMATCH,
            Mismatch::Missing(_) => SIGNATURE_MISSING,
        }
    }
}

/// True for `[A-Za-z0-9_-]{1,128}`.
pub fn is_safe_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn hex_prefixed(s: &str, digits: usize) -> bool {
    s.len() == digits + 2 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn lower_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A model id as a catalog lists it: `[A-Za-z0-9._/-]`, 1 to 200
/// characters (it goes into a code span and a URL query).
fn model_ok(model: &str) -> bool {
    (1..=200).contains(&model.len())
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
}

/// Percent-encodes everything but the unreserved characters.
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn one_beside<'a>(
    v: &'a ValidatedManifest,
    dir: &str,
    kind: &str,
) -> Result<&'a ValidatedEvidence, Mismatch> {
    let found: Vec<&ValidatedEvidence> = v
        .evidence
        .iter()
        .filter(|e| e.kind == kind && parent(&e.file.path) == dir)
        .collect();
    match found.as_slice() {
        [one] if one.visibility == Visibility::Private => Ok(one),
        [_] => Err(Mismatch::Mismatch(format!(
            "the {kind} entry is not private"
        ))),
        [] => Err(Mismatch::Mismatch(format!(
            "no {kind} entry beside the signature"
        ))),
        _ => Err(Mismatch::Mismatch(format!(
            "more than one {kind} entry beside the signature"
        ))),
    }
}

/// Rechecks the manifest's `tee_signature` entry against the
/// generation's frozen direct and signed models. `Ok(None)`: no entry and
/// none required.
pub fn recheck(
    v: &ValidatedManifest,
    direct_models: &[String],
    signed_models: &[String],
) -> Result<Option<SignedSection>, Mismatch> {
    let sigs: Vec<&ValidatedEvidence> = v
        .evidence
        .iter()
        .filter(|e| e.kind == KIND_SIGNATURE)
        .collect();
    let sig = match sigs.as_slice() {
        [] if signed_models.is_empty() => return Ok(None),
        [] => {
            return Err(Mismatch::Missing(
                "the listing signs a model call and the run's manifest has no tee_signature entry"
                    .into(),
            ));
        }
        [one] => *one,
        _ => {
            return Err(Mismatch::Mismatch(
                "the manifest lists more than one tee_signature entry".into(),
            ));
        }
    };
    if sig.visibility != Visibility::Public {
        return Err(Mismatch::Mismatch(
            "the tee_signature entry is not public".into(),
        ));
    }
    let dir = parent(&sig.file.path);
    let request = one_beside(v, dir, KIND_REQUEST)?;
    let response = one_beside(v, dir, KIND_RESPONSE)?;
    let s: SignatureFile = serde_json::from_slice(&sig.file.bytes)
        .map_err(|e| Mismatch::Mismatch(format!("signature.json does not parse: {e}")))?;
    let bad = |what: &str| Err(Mismatch::Mismatch(format!("{what} is malformed")));
    if s.version != 1 {
        return bad("the version");
    }
    if !is_safe_id(&s.completion_id) {
        return bad("the completion id");
    }
    if !model_ok(&s.model) {
        return bad("the model");
    }
    if !hex_prefixed(&s.signing_address, 40) {
        return bad("the signing address");
    }
    if !hex_prefixed(&s.signature, 130) {
        return bad("the signature");
    }
    if !lower_hex64(&s.request_sha256) || !lower_hex64(&s.response_sha256) {
        return bad("a hash");
    }
    if s.signing_algo != "ecdsa" {
        return bad("the signing algorithm");
    }
    if s.signature_kind != "provider_tee" {
        return Err(Mismatch::Mismatch(format!(
            "the signature kind is {}, not provider_tee: the response is not bound to a model TEE",
            if s.signature_kind == "gateway" {
                "gateway"
            } else {
                "unknown"
            }
        )));
    }
    if !direct_models.contains(&s.model) {
        return Err(Mismatch::Mismatch(format!(
            "the model {} is not one of the generation's direct models",
            s.model
        )));
    }
    if !signed_models.is_empty() && !signed_models.contains(&s.model) {
        return Err(Mismatch::Missing(format!(
            "the signed model {} is not one the listing signs",
            s.model
        )));
    }
    let request_sha = sha256_hex(&request.file.bytes);
    let response_sha = sha256_hex(&response.file.bytes);
    if request_sha != s.request_sha256 || response_sha != s.response_sha256 {
        return Err(Mismatch::Mismatch(
            "the stored request or response bytes do not match their hashes".into(),
        ));
    }
    if s.signed_text != format!("{}:{request_sha}:{response_sha}", s.model) {
        return Err(Mismatch::Mismatch(
            "the signed text does not bind the stored bytes".into(),
        ));
    }
    Ok(Some(SignedSection {
        markdown: section(&s),
        request: request.file.bytes.clone(),
        response: response.file.bytes.clone(),
    }))
}

/// The base64url (no padding) receipt of a checked signature.
pub fn receipt(s: &SignatureFile) -> String {
    let json = serde_json::to_vec(&Receipt {
        completion_id: &s.completion_id,
        model: &s.model,
        signature_kind: &s.signature_kind,
        signing_algo: &s.signing_algo,
        signing_address: &s.signing_address,
        signed_text: &s.signed_text,
        signature: &s.signature,
    })
    .expect("the receipt serializes");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

/// The receipt link of a checked signature.
pub fn receipt_link(s: &SignatureFile) -> String {
    format!(
        "{VERIFY_URL}?model={}#receipt={}",
        url_encode(&s.model),
        receipt(s)
    )
}

/// The section `Response signature` (every field in a code span; the
/// fields were checked, so none holds a backtick). The receipt link is a
/// bare URL alone on its line, never wrapped in Markdown link syntax:
/// a buyer who sees the raw text (a terminal, a plain-text view) would
/// otherwise copy or click the closing `)` with it, and the Verification
/// Center rejects such a receipt as invalid.
fn section(s: &SignatureFile) -> String {
    format!(
        "## Response signature\n\n\
         The signed call ran on the NEAR AI Cloud model `{model}` in a model TEE, which signed its response (`provider_tee`, ECDSA).\n\n\
         - Completion id: `{id}`\n\
         - Model: `{model}`\n\
         - Signing address: `{address}`\n\
         - Request SHA-256: `{req}`\n\
         - Response SHA-256: `{resp}`\n\
         - Signed text: `{text}`\n\
         - Signature: `{sig}`\n\n\
         What you can verify: the signature binds a response with the response hash above, produced in a model TEE with the signing address above. The request and response bytes stay with the owner, so you cannot recompute the hashes yourself; the proof is that a model TEE signed a response with that hash, not that this deliverable was derived from it.\n\n\
         Check the signature in the NEAR AI Verification Center: open the link below, whole. The page recovers the signer of the signature in the browser and checks it against the model's current TEE attestation; the receipt stays in the URL fragment.\n\n\
         {link}\n",
        model = s.model,
        id = s.completion_id,
        address = s.signing_address,
        req = s.request_sha256,
        resp = s.response_sha256,
        text = s.signed_text,
        sig = s.signature,
        link = receipt_link(s),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ValidatedFile;

    const MODEL: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";

    fn file(path: &str, bytes: &[u8]) -> ValidatedFile {
        ValidatedFile {
            path: path.into(),
            sha256: sha256_hex(bytes),
            bytes: bytes.to_vec(),
        }
    }

    fn sig_file(req: &[u8], resp: &[u8]) -> SignatureFile {
        let (r, p) = (sha256_hex(req), sha256_hex(resp));
        SignatureFile {
            version: 1,
            completion_id: "chatcmpl-1".into(),
            model: MODEL.into(),
            signature_kind: "provider_tee".into(),
            signing_algo: "ecdsa".into(),
            signing_address: format!("0x{}", "ab".repeat(20)),
            signed_text: format!("{MODEL}:{r}:{p}"),
            signature: format!("0x{}", "1b".repeat(65)),
            request_sha256: r,
            response_sha256: p,
        }
    }

    fn manifest(sig: &SignatureFile, req: &[u8], resp: &[u8]) -> ValidatedManifest {
        let md = b"# Digest\n";
        let ev = |path: &str, bytes: &[u8], kind: &str, visibility| ValidatedEvidence {
            file: file(path, bytes),
            kind: kind.into(),
            visibility,
        };
        ValidatedManifest {
            markdown: file("deliverable.md", md),
            markdown_text: "# Digest\n".into(),
            files: vec![],
            evidence: vec![
                ev(
                    "evidence/signature.json",
                    &serde_json::to_vec(sig).unwrap(),
                    KIND_SIGNATURE,
                    Visibility::Public,
                ),
                ev(
                    "evidence/request.bin",
                    req,
                    KIND_REQUEST,
                    Visibility::Private,
                ),
                ev(
                    "evidence/response.bin",
                    resp,
                    KIND_RESPONSE,
                    Visibility::Private,
                ),
                ev(
                    "evidence/report.json",
                    b"{}",
                    "tee_model_report",
                    Visibility::Private,
                ),
            ],
        }
    }

    fn models() -> (Vec<String>, Vec<String>) {
        (
            vec!["openai/privacy-filter".into(), MODEL.into()],
            vec![MODEL.into()],
        )
    }

    #[test]
    fn a_valid_entry_renders_the_section_and_the_receipt() {
        let (req, resp) = (b"{\"a\":1}".as_slice(), b"{\"b\":2}".as_slice());
        let s = sig_file(req, resp);
        let (d, sm) = models();
        let out = recheck(&manifest(&s, req, resp), &d, &sm).unwrap().unwrap();
        assert_eq!(out.request, req);
        assert_eq!(out.response, resp);
        let md = &out.markdown;
        assert!(md.starts_with("## Response signature\n"), "{md}");
        for field in [
            &s.completion_id,
            &s.model,
            &s.signing_address,
            &s.request_sha256,
            &s.response_sha256,
            &s.signed_text,
            &s.signature,
        ] {
            assert!(md.contains(&format!("`{field}`")), "{field}: {md}");
        }
        assert!(md.contains("stay with the owner"), "{md}");
        assert!(md.contains("cannot recompute the hashes"), "{md}");
        let link = format!("{VERIFY_URL}?model=deepseek-ai%2FDeepSeek-V4.1-Flash-1017#receipt=");
        let at = md.find(&link).expect("the receipt link");
        let rest = &md[at + link.len()..];
        let encoded = &rest[..rest.find('\n').unwrap()];
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .unwrap();
        let text = String::from_utf8(json).unwrap();
        assert_eq!(
            text,
            format!(
                "{{\"completionId\":\"chatcmpl-1\",\"model\":\"{MODEL}\",\"signatureKind\":\"provider_tee\",\"signingAlgo\":\"ecdsa\",\"signingAddress\":\"{}\",\"signedText\":\"{}\",\"signature\":\"{}\"}}",
                s.signing_address, s.signed_text, s.signature
            )
        );
        // The private bytes never reach the section.
        assert!(!md.contains("{\"b\":2}"));
    }

    /// The link of the real paid hire `dbfbe061` (2026-11-13), as the
    /// Verification Center showed it valid on 2026-11-14.
    const DBFBE061_LINK: &str = include_str!("../tests/fixtures/receipt-link-dbfbe061.txt");

    /// The Verification Center's own receipt parser (its page bundle, read
    /// 2026-11-14): the fragment as URL search parameters, exactly one
    /// `receipt`, at most 32768 characters of `[A-Za-z0-9_-]` with at most
    /// two `=`, base64url JSON, the six keys as non-empty strings and a
    /// `signatureKind` of `provider_tee` or `gateway`. Anything else is
    /// "The response receipt is invalid."
    fn page_accepts(link: &str) -> Result<serde_json::Value, String> {
        let (_, fragment) = link.split_once('#').ok_or("no fragment")?;
        let receipts: Vec<&str> = fragment
            .split('&')
            .filter_map(|p| p.strip_prefix("receipt="))
            .collect();
        let [r] = receipts.as_slice() else {
            return Err(format!("{} receipts", receipts.len()));
        };
        let body = r.trim_end_matches('=');
        let well_formed = !r.is_empty()
            && r.len() <= 32768
            && r.len() - body.len() <= 2
            && body
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !well_formed {
            return Err(format!("the receipt {r:?} is not base64url"));
        }
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
        for key in [
            "completionId",
            "model",
            "signingAlgo",
            "signingAddress",
            "signedText",
            "signature",
        ] {
            if !v[key].as_str().is_some_and(|s| !s.is_empty()) {
                return Err(format!("{key} is missing"));
            }
        }
        if !matches!(
            v["signatureKind"].as_str(),
            Some("provider_tee" | "gateway")
        ) {
            return Err("signatureKind is neither provider_tee nor gateway".into());
        }
        Ok(v)
    }

    fn dbfbe061() -> SignatureFile {
        SignatureFile {
            version: 1,
            completion_id: "chatcmpl-ee1d004e-2109-4673-85a9-580682bb51f6".into(),
            model: MODEL.into(),
            signature_kind: "provider_tee".into(),
            signing_algo: "ecdsa".into(),
            signing_address: "0xfe0071895500a9755c18044948ab4f4e1a7f337e".into(),
            signed_text: format!(
                "{MODEL}:3b2edf7af715f3105641fe1266b906b73fd6a4191a1b857dc3f30c7a441cb79b:214bc08dd9f3897236841b6247d66d64e6f132003cd5de1683e27a085062e879"
            ),
            signature: "0x7b7ae52a4809d5f1214d2d5340d39a0b4cba0709a3abda0dcda651c61878798b5df8714fc83eca0610d173d33e514111ad3157922dd77fcce5a3db26f0c36c221c".into(),
            request_sha256: "3b2edf7af715f3105641fe1266b906b73fd6a4191a1b857dc3f30c7a441cb79b".into(),
            response_sha256: "214bc08dd9f3897236841b6247d66d64e6f132003cd5de1683e27a085062e879".into(),
        }
    }

    #[test]
    fn the_dbfbe061_link_is_the_one_the_verification_center_accepted() {
        let s = dbfbe061();
        assert_eq!(receipt_link(&s), DBFBE061_LINK.trim_end());
        let v = page_accepts(&receipt_link(&s)).unwrap();
        assert_eq!(v["completionId"], s.completion_id.as_str());
        assert_eq!(v["signedText"], s.signed_text.as_str());
    }

    #[test]
    fn the_section_puts_the_link_alone_on_its_line() {
        let s = dbfbe061();
        let md = section(&s);
        let line = md
            .lines()
            .find(|l| l.contains("#receipt="))
            .expect("the link line");
        assert_eq!(line, receipt_link(&s), "{md}");
        assert!(page_accepts(line).is_ok());
        // A closing parenthesis glued to the link (a raw Markdown link seen
        // as plain text) is what the page rejects as invalid.
        assert!(page_accepts(&format!("{line})")).is_err());
        assert!(!md.contains("](http"), "{md}");
    }

    #[test]
    fn an_edited_response_is_a_mismatch() {
        let s = sig_file(b"req", b"resp");
        let (d, sm) = models();
        let err = recheck(&manifest(&s, b"req", b"edited"), &d, &sm).unwrap_err();
        assert_eq!(err.code(), SIGNATURE_MISMATCH);
    }

    #[test]
    fn a_gateway_kind_or_a_model_outside_the_frozen_list_is_a_mismatch() {
        let (d, sm) = models();
        let mut s = sig_file(b"req", b"resp");
        s.signature_kind = "gateway".into();
        let err = recheck(&manifest(&s, b"req", b"resp"), &d, &sm).unwrap_err();
        assert_eq!(err.code(), SIGNATURE_MISMATCH);
        assert!(err.to_string().contains("gateway"), "{err}");
        let s = sig_file(b"req", b"resp");
        let err = recheck(
            &manifest(&s, b"req", b"resp"),
            &["openai/privacy-filter".into()],
            &[],
        )
        .unwrap_err();
        assert_eq!(err.code(), SIGNATURE_MISMATCH);
    }

    #[test]
    fn a_markdown_link_in_a_field_is_a_mismatch() {
        let (d, sm) = models();
        for edit in [
            |s: &mut SignatureFile| s.completion_id = "[x](https://e.x)".into(),
            |s: &mut SignatureFile| s.signing_address = "0x`".into(),
            |s: &mut SignatureFile| s.signature = "0xab".into(),
            |s: &mut SignatureFile| s.request_sha256 = "AB".repeat(32),
            |s: &mut SignatureFile| s.signing_algo = "ed25519".into(),
        ] {
            let mut s = sig_file(b"req", b"resp");
            edit(&mut s);
            let err = recheck(&manifest(&s, b"req", b"resp"), &d, &sm).unwrap_err();
            assert_eq!(err.code(), SIGNATURE_MISMATCH, "{err}");
        }
    }

    #[test]
    fn a_signed_listing_without_an_entry_is_missing() {
        let (d, sm) = models();
        let s = sig_file(b"req", b"resp");
        let mut m = manifest(&s, b"req", b"resp");
        m.evidence.clear();
        let err = recheck(&m, &d, &sm).unwrap_err();
        assert_eq!(err.code(), SIGNATURE_MISSING);
        assert_eq!(recheck(&m, &d, &[]), Ok(None));
    }

    #[test]
    fn a_request_in_another_directory_is_a_mismatch() {
        let (d, sm) = models();
        let s = sig_file(b"req", b"resp");
        let mut m = manifest(&s, b"req", b"resp");
        m.evidence[1].file.path = "other/request.bin".into();
        let err = recheck(&m, &d, &sm).unwrap_err();
        assert!(err.to_string().contains("tee_request"), "{err}");
    }
}
