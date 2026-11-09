//! A string secret whose formatting never shows the value.

use std::fmt;

/// Text shown instead of a secret value.
pub const REDACTED: &str = "<redacted>";

/// A secret value (API key, token, private key). `Debug` and `Display`
/// print `<redacted>`; the value is reachable only through [`Secret::expose`].
/// It deliberately implements neither `Serialize` nor `Deserialize`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for the one place that must send it (a request header).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_never_shows_the_value() {
        let s = Secret::new("sk-abc-123");
        assert_eq!(format!("{s}"), REDACTED);
        assert_eq!(format!("{s:?}"), REDACTED);
        assert_eq!(format!("{s:#?}"), REDACTED);
        assert!(!format!("{:?}", Some(&s)).contains("sk-abc"));
        assert_eq!(s.expose(), "sk-abc-123");
    }
}
