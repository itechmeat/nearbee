//! Bounding and redaction of text that may end up in errors or logs.

use crate::secret::{REDACTED, Secret};

/// Maximum length in bytes of an error detail.
pub const DETAIL_LIMIT: usize = 2048;

/// Removes every configured secret value (longest first, so a secret that
/// contains another is removed whole) and every `Bearer <token>`, quoted or
/// not, from `text`, then cuts it to [`DETAIL_LIMIT`] bytes on a character
/// boundary.
pub fn redact_detail(text: &str, secrets: &[Secret]) -> String {
    let mut values: Vec<&str> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    let mut out = text.to_owned();
    for v in values {
        out = out.replace(v, REDACTED);
    }
    out = redact_bearer(&out);
    truncate(&out, DETAIL_LIMIT)
}

fn redact_bearer(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(pos) = find_ci(rest, "bearer ") else {
            out.push_str(rest);
            return out;
        };
        let mut start = pos + "bearer ".len();
        // A quoted token: keep the opening quote (plain or JSON-escaped)
        // and remove what follows it.
        let opening = rest[start..]
            .bytes()
            .take_while(|b| matches!(b, b'"' | b'\'' | b'\\'))
            .count();
        start += opening;
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '\\'))
            .unwrap_or(tail.len());
        if end > 0 {
            out.push_str(REDACTED);
        }
        rest = &tail[end..];
    }
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack.char_indices().map(|(i, _)| i).find(|&i| {
        haystack
            .get(i..i + needle.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(needle))
    })
}

/// Cuts `text` to at most `limit` bytes without splitting a character.
pub fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_configured_secrets_and_bearer_tokens() {
        let s = Secret::new("apb_s3cr3t");
        let text = "failed: key apb_s3cr3t; header Authorization: Bearer xyz.123, again bearer abc";
        let out = redact_detail(text, &[s]);
        assert!(!out.contains("apb_s3cr3t"));
        assert!(!out.contains("xyz.123"));
        assert!(!out.contains("abc"));
        assert!(out.contains("Bearer <redacted>"));
    }

    #[test]
    fn a_secret_that_contains_another_is_removed_whole() {
        // The shorter secret comes first and is a substring of the longer.
        let short = Secret::new("abc123");
        let long = Secret::new("zz-abc123-tail-9876");
        let out = redact_detail("got zz-abc123-tail-9876 here", &[short, long]);
        assert!(!out.contains("tail-9876"), "{out}");
        assert!(!out.contains("zz-"), "{out}");
        assert_eq!(out, "got <redacted> here");
    }

    #[test]
    fn quoted_bearer_token_is_removed() {
        for text in [
            r#"{"authorization":"Bearer \"tok_quoted_1\""}"#,
            "auth: Bearer \"tok_quoted_1\" end",
            "auth: Bearer 'tok_quoted_1' end",
        ] {
            let out = redact_detail(text, &[]);
            assert!(!out.contains("tok_quoted_1"), "{text} -> {out}");
        }
    }

    #[test]
    fn cuts_to_the_limit_on_a_char_boundary() {
        let text = "é".repeat(DETAIL_LIMIT);
        let out = redact_detail(&text, &[]);
        assert!(out.len() <= DETAIL_LIMIT);
        assert!(out.len() >= DETAIL_LIMIT - 1);
    }
}
