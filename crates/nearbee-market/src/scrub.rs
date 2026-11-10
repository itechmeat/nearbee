//! Fixture scrubbing for the capture example: every token, capability
//! link segment and presigned query is replaced before a recorded market
//! answer is written to disk, so the fixtures carry no live credential.

use nearbee_core::secret::{REDACTED, Secret};

/// Replaces every `aat_`-shaped token with `<redacted>`.
pub fn scrub_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("aat_") {
        let tail = &rest[pos + 4..];
        let hex: String = tail
            .chars()
            .take(32)
            .take_while(char::is_ascii_hexdigit)
            .collect();
        if hex.len() == 32 {
            out.push_str(&rest[..pos]);
            out.push_str("<redacted>");
            rest = &tail[32..];
        } else {
            out.push_str(&rest[..pos + 4]);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// Replaces every non-empty segment after `/files/` with `<redacted>` (the
/// capability token of a file link).
pub fn scrub_capability_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("/files/") {
        let start = pos + "/files/".len();
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace() || c == '"')
            .unwrap_or(tail.len());
        if end > 0 {
            out.push_str("<redacted>");
            rest = &tail[end..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// Strips the query part of any `https://` URL (a presigned upload URL's
/// signature never survives in a fixture).
pub fn scrub_url_queries(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("https://") {
        let after = &rest[pos..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == ')')
            .unwrap_or(after.len());
        let url = &after[..end];
        match url.split_once('?') {
            Some((base, _)) => {
                out.push_str(&rest[..pos]);
                out.push_str(base);
                out.push_str("?<redacted>");
            }
            None => out.push_str(&rest[..pos + end]),
        }
        rest = &rest[pos + end..];
    }
    out.push_str(rest);
    out
}

/// Scrubs a recorded answer with every rule, plus the caller's secrets.
/// The body is never length-cut: a fixture must stay valid JSON for the
/// replay tests (the redaction of error details truncates; this does not).
pub fn scrub_fixture(text: &str, secrets: &[Secret]) -> String {
    let scrubbed = scrub_url_queries(&scrub_capability_links(&scrub_tokens(text)));
    let mut values: Vec<&str> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    let mut out = scrubbed;
    for v in values {
        out = out.replace(v, REDACTED);
    }
    out
}

/// One transcript entry of the file-trio capture, built with serde_json so
/// the line is always valid JSON: the status is the numeric code and the
/// body is a JSON string (ocr2 6).
pub fn transcript_entry(step: &str, status: u16, body: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "step": step,
        "status": status,
        "body": body,
    }))
    .expect("transcript entry serializes")
}

/// Joins transcript entries into a JSON array text.
pub fn transcript_json(entries: &[String]) -> String {
    let value: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| serde_json::from_str(e).expect("entry parses"))
        .collect();
    serde_json::to_string_pretty(&value).expect("transcript serializes") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_replaced_whole() {
        let text = r#"{"token":"aat_0123456789abcdef0123456789abcdef","note":"aat_ is a prefix"}"#;
        let out = scrub_tokens(text);
        assert!(!out.contains("0123456789abcdef0123456789abcdef"));
        assert!(out.contains(r#""token":"<redacted>""#));
        // The prose prefix survives: only whole tokens are replaced.
        assert!(out.contains("aat_ is a prefix"));
    }

    #[test]
    fn capability_links_lose_their_segment() {
        let out = scrub_capability_links("see https://m.near.ai/files/deadbeef00/d.md there");
        assert!(!out.contains("deadbeef00"));
        assert!(out.contains("/files/<redacted>/d.md"));
    }

    #[test]
    fn url_queries_are_stripped() {
        let out =
            scrub_url_queries(r#"{"uploadUrl":"https://up.example/f?X-Amz-Signature=abc&x=1"}"#);
        assert!(!out.contains("Signature"));
        assert!(out.contains("https://up.example/f?<redacted>"));
    }

    /// A long body keeps its full length: the fixture must stay valid
    /// JSON (the 2 KiB detail bound is for errors, not for fixtures).
    #[test]
    fn scrub_fixture_never_cuts_the_body() {
        let secret = Secret::new("super-secret-api-key-value");
        let pad = "x".repeat(5000);
        let text = format!(r#"{{"note":"{pad}","token":"aat_0123456789abcdef0123456789abcdef"}}"#);
        let out = scrub_fixture(&text, &[secret]);
        assert!(out.len() > 5000, "{} bytes", out.len());
        assert!(serde_json::from_str::<serde_json::Value>(&out).is_ok());
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn scrub_fixture_applies_every_rule_and_the_caller_secrets() {
        let secret = Secret::new("super-secret-api-key-value");
        let text = "token aat_0123456789abcdef0123456789abcdef link /files/abc123/x url https://u.example/p?s=1 key super-secret-api-key-value".to_string();
        let out = scrub_fixture(&text, &[secret]);
        assert!(!out.contains("0123456789abcdef"));
        assert!(!out.contains("abc123"));
        assert!(!out.contains("s=1"));
        assert!(!out.contains("super-secret"));
        assert!(out.contains("<redacted>"));
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::*;

    /// The transcript entry is one valid JSON line with a numeric status.
    #[test]
    fn the_transcript_entry_is_valid_json_with_a_numeric_status() {
        let line = transcript_entry("reserve", 200, "{\"fileId\":\"f\"}");
        let v: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(v["step"], "reserve");
        assert_eq!(v["status"], 200, "the status is the numeric code");
        assert_eq!(v["body"], "{\"fileId\":\"f\"}");
    }

    /// A whole transcript parses as a JSON array with no trailing comma.
    #[test]
    fn the_whole_transcript_parses_as_an_array() {
        let entries = vec![
            transcript_entry("reserve", 200, "{}"),
            transcript_entry("put", 200, ""),
            transcript_entry("delete", 204, ""),
        ];
        let text = transcript_json(&entries);
        let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON array");
        assert_eq!(v.as_array().unwrap().len(), 3);
        assert_eq!(v[2]["step"], "delete");
    }
}
