//! Listing files: the YAML a publish reads, its validation, and the
//! resolved record of a published listing.
//!
//! Configuration and listing files name agents by handle only; a
//! token-like value in one is refused at load without printing the value.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::credentials::find_token_like;
use crate::redact::truncate;

/// The market's category enum (`docs/market.md`, verified 2026-11-09).
pub const CATEGORIES: [&str; 14] = [
    "automation",
    "creative",
    "customer_support",
    "data",
    "dev_tools",
    "events",
    "finance",
    "legal_compliance",
    "marketing",
    "operations",
    "research",
    "sales",
    "travel",
    "other",
];

/// Field limits of a listing file.
pub const TAGS_MAX: usize = 10;
pub const TAG_MAX: usize = 40;
pub const DESCRIPTION_MAX: usize = 200;
pub const HANDLE_MAX: usize = 30;

/// A listing file as written in the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub handle: String,
    pub name: String,
    pub source_playbook: String,
    /// Decimal string in USD, never a float.
    pub price_usd: String,
    pub sla_seconds: u64,
    pub category: String,
    pub tags: Vec<String>,
    pub description: String,
    pub bio_markdown: String,
}

/// A listing file failure. Errors never carry a token value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListingError {
    #[error("listing file {path}: {message}")]
    Parse { path: String, message: String },
    #[error(
        "listing file {path}: category `{found}` is not one of the market's categories: {allowed}"
    )]
    Category {
        path: String,
        found: String,
        allowed: String,
    },
    #[error(
        "listing file {path}: price_usd must be a decimal string in quotes (like \"0.50\"), not a number"
    )]
    PriceNotString { path: String },
    #[error(
        "listing file: handle `{1}` is not a market handle (3 to 30 characters, [a-z][a-z0-9-]*): {0}"
    )]
    Handle(String, String),
    #[error(
        "listing file {path}: tags must be at most {TAGS_MAX} of 1 to {TAG_MAX} characters each"
    )]
    Tags { path: String },
    #[error("listing file {path}: description must be at most {DESCRIPTION_MAX} characters")]
    DescriptionTooLong { path: String },
    #[error(
        "listing file {path} holds an agent token; tokens belong in the credentials store, not a listing"
    )]
    Token { path: String },
}

/// Parses and validates a listing file's text.
pub fn parse(text: &str, path: &Path) -> Result<Listing, ListingError> {
    let path = path.display().to_string();
    if find_token_like(text).is_some() {
        return Err(ListingError::Token { path });
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        handle: String,
        name: String,
        source_playbook: String,
        price_usd: RawPrice,
        sla_seconds: u64,
        category: String,
        tags: Vec<String>,
        description: String,
        bio_markdown: String,
    }
    /// A price that accepts only a YAML string, never a number.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawPrice {
        Str(String),
        /// Anything else (a number, a mapping): refused by name.
        Other(#[allow(dead_code)] serde_json::Value),
    }
    let raw: Raw = yaml_serde::from_str(text).map_err(|e| ListingError::Parse {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let price_usd = match raw.price_usd {
        RawPrice::Str(s) => {
            let parsed = s.parse::<rust_decimal::Decimal>();
            let ok =
                parsed.is_ok_and(|p| p.is_sign_positive() && !p.is_zero() && p.round_dp(6) == p);
            if !ok {
                return Err(ListingError::Parse {
                    path: path.clone(),
                    message: format!(
                        "price_usd `{s}` is not a positive decimal amount with at most 6 decimal places"
                    ),
                });
            }
            s
        }
        RawPrice::Other(_) => return Err(ListingError::PriceNotString { path }),
    };
    if !handle_ok(&raw.handle) {
        return Err(ListingError::Handle(path, raw.handle));
    }
    if !CATEGORIES.contains(&raw.category.as_str()) {
        return Err(ListingError::Category {
            allowed: CATEGORIES.join(", "),
            found: raw.category,
            path,
        });
    }
    let tags_ok = raw.tags.len() <= TAGS_MAX
        && raw
            .tags
            .iter()
            .all(|t| !t.is_empty() && t.chars().count() <= TAG_MAX);
    if !tags_ok {
        return Err(ListingError::Tags { path });
    }
    if raw.description.chars().count() > DESCRIPTION_MAX {
        return Err(ListingError::DescriptionTooLong { path });
    }
    Ok(Listing {
        handle: raw.handle,
        name: raw.name,
        source_playbook: raw.source_playbook,
        price_usd,
        sla_seconds: raw.sla_seconds,
        category: raw.category,
        tags: raw.tags,
        description: truncate(&raw.description, DESCRIPTION_MAX * 4 + 1),
        bio_markdown: raw.bio_markdown,
    })
}

/// A market handle: 3 to 30 characters, `[a-z][a-z0-9-]*`.
pub fn handle_ok(handle: &str) -> bool {
    let n = handle.len();
    (3..=HANDLE_MAX).contains(&n)
        && handle.starts_with(|c: char| c.is_ascii_lowercase())
        && handle
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The resolved listing: what a publish installed and asked the market
/// for, recorded at `.nearbee/listings/<handle>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedListing {
    pub handle: String,
    pub agent_id: String,
    /// The apb playbook id the listing is backed by (`<handle>`).
    pub playbook_id: String,
    /// The apb playbook version installed for this listing.
    pub playbook_version: String,
    /// SHA-256 of the materialised playbook text.
    pub playbook_sha256: String,
    /// Decimal string in USD.
    pub price_usd: String,
    pub sla_seconds: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: time::OffsetDateTime,
}

/// Reads and writes resolved listings under `<state_dir>/listings/`.
pub fn resolved_path(state_dir: &Path, handle: &str) -> std::path::PathBuf {
    state_dir.join("listings").join(format!("{handle}.json"))
}

pub fn read_resolved(state_dir: &Path, handle: &str) -> Result<Option<ResolvedListing>, String> {
    if !handle_ok(handle) {
        return Err(format!("invalid listing handle: {handle:?}"));
    }
    let p = resolved_path(state_dir, handle);
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{}: {e}", p.display()))
}

pub fn write_resolved(state_dir: &Path, r: &ResolvedListing) -> Result<(), String> {
    if !handle_ok(&r.handle) {
        return Err(format!("invalid listing handle: {:?}", r.handle));
    }
    let p = resolved_path(state_dir, &r.handle);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Atomic and durable: the record is written to a temp file in the
    // same directory, fsynced, renamed over the destination and the
    // directory fsynced too (R2-7), so a crash or a power loss never
    // leaves a torn or empty listing behind.
    let mut tmp = tempfile::NamedTempFile::new_in(p.parent().unwrap())
        .map_err(|e| format!("{}: {e}", p.display()))?;
    use std::io::Write as _;
    tmp.write_all(
        serde_json::to_string_pretty(r)
            .expect("resolved listing serializes")
            .as_bytes(),
    )
    .map_err(|e| format!("{}: {e}", p.display()))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| format!("{}: {e}", p.display()))?;
    tmp.persist(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    if let Some(dir) = p.parent() {
        std::fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

impl fmt::Display for Listing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {} USD)",
            self.handle, self.category, self.price_usd
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> String {
        r#"
handle: repo-audit-fast
name: "Repo audit (fast)"
source_playbook: playbooks/first-hire/repo-audit-fast.yaml
price_usd: "0.50"
sla_seconds: 86400
category: dev_tools
tags: ["nearbee", "example"]
description: "Audits a public repository and returns a short Markdown report."
bio_markdown: "A Nearbee example agent."
"#
        .to_owned()
    }

    #[test]
    fn a_well_formed_listing_parses() {
        let l = parse(&text(), Path::new("listing.yaml")).unwrap();
        assert_eq!(l.handle, "repo-audit-fast");
        assert_eq!(l.price_usd, "0.50");
        assert_eq!(l.category, "dev_tools");
        assert_eq!(l.tags.len(), 2);
    }

    #[test]
    fn a_category_outside_the_enum_names_the_field_and_the_allowed_values() {
        let bad = text().replace("dev_tools", "games");
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ListingError::Category { .. }), "{msg}");
        assert!(msg.contains("games") && msg.contains("research"), "{msg}");
    }

    #[test]
    fn a_price_as_a_float_is_refused_asking_for_a_decimal_string() {
        let bad = text().replace("\"0.50\"", "0.5");
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ListingError::PriceNotString { .. }), "{msg}");
        assert!(msg.contains("decimal string"), "{msg}");
        // A non-decimal string is refused too.
        let bad = text().replace("\"0.50\"", "\"free\"");
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        assert!(matches!(err, ListingError::Parse { .. }), "{err}");
    }

    #[test]
    fn unknown_keys_are_refused() {
        let bad = text().replace("bio_markdown:", "bio_markdown_x:");
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        assert!(err.to_string().contains("bio_markdown_x"), "{err}");
    }

    #[test]
    fn a_token_in_a_listing_file_is_refused_without_the_value() {
        let tok = "aat_0123456789abcdef0123456789abcdef";
        let bad = text().replace("Nearbee example agent.", &format!("key {tok}"));
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ListingError::Token { .. }), "{msg}");
        assert!(!msg.contains(tok), "{msg}");
    }

    #[test]
    fn tag_and_description_limits_are_enforced() {
        let quoted: Vec<String> = (0..11).map(|_| "\"t\"".to_owned()).collect();
        let bad = text().replace(
            "[\"nearbee\", \"example\"]",
            &format!("[{}]", quoted.join(", ")),
        );
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        assert!(matches!(err, ListingError::Tags { .. }), "{err}");
        let bad = text().replace(
            "[\"nearbee\", \"example\"]",
            &format!("[\"{}\"]", "t".repeat(41)),
        );
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        assert!(matches!(err, ListingError::Tags { .. }), "{err}");
        let bad = text().replace(
            "Audits a public repository and returns a short Markdown report.",
            &"d".repeat(201),
        );
        let err = parse(&bad, Path::new("listing.yaml")).unwrap_err();
        assert!(
            matches!(err, ListingError::DescriptionTooLong { .. }),
            "{err}"
        );
    }

    #[test]
    fn handles_are_checked_against_the_market_shape() {
        for ok in ["abc", "repo-audit-fast", "a1-b2"] {
            assert!(handle_ok(ok), "{ok}");
        }
        for bad in ["ab", "Abc", "-abc", "a_b", "", "a".repeat(31).as_str()] {
            assert!(!handle_ok(bad), "{bad}");
        }
        let l = parse(&text(), Path::new("l.yaml")).unwrap();
        assert_eq!(l.to_string(), "repo-audit-fast (dev_tools, 0.50 USD)");
    }

    #[test]
    fn resolved_listings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let r = ResolvedListing {
            handle: "repo-audit-fast".into(),
            agent_id: "ag_1".into(),
            playbook_id: "repo-audit-fast".into(),
            playbook_version: "1.0.0".into(),
            playbook_sha256: crate::journal::sha256_hex(b"playbook"),
            price_usd: "0.50".into(),
            sla_seconds: 86_400,
            published_at: time::OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        };
        write_resolved(dir.path(), &r).unwrap();
        let got = read_resolved(dir.path(), "repo-audit-fast")
            .unwrap()
            .unwrap();
        assert_eq!(got, r);
        assert!(read_resolved(dir.path(), "other").unwrap().is_none());
        assert_eq!(
            resolved_path(dir.path(), "repo-audit-fast"),
            dir.path().join("listings/repo-audit-fast.json")
        );
    }
}

#[cfg(test)]
mod resolved_tests {
    use super::*;

    fn sample(handle: &str) -> ResolvedListing {
        ResolvedListing {
            handle: handle.to_owned(),
            agent_id: "ag_1".into(),
            playbook_id: handle.to_owned(),
            playbook_version: "1.0.0".into(),
            playbook_sha256: "ab".into(),
            price_usd: "0.50".into(),
            sla_seconds: 3600,
            published_at: time::OffsetDateTime::from_unix_timestamp(0).unwrap(),
        }
    }

    /// A listing handle never smuggles a path or an empty name (ocr2 13).
    #[test]
    fn a_listing_handle_cannot_carry_a_path() {
        for bad in ["", "../x", "a/b", "..", "a b", "A"] {
            assert!(!handle_ok(bad), "{bad:?} passed");
        }
        for good in ["repo-audit-fast", "abc", "a1-2b"] {
            assert!(handle_ok(good), "{good:?} refused");
        }
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../evil", "a/b", ""] {
            let err = write_resolved(dir.path(), &sample(bad)).unwrap_err();
            assert!(err.contains("invalid listing handle"), "{err}");
        }
        let err = read_resolved(dir.path(), "../evil").unwrap_err();
        assert!(err.contains("invalid listing handle"), "{err}");
        assert!(!dir.path().join("evil").exists());
    }

    /// The resolved record is written atomically: a crash mid-write never
    /// leaves a torn listing (ocr2 13).
    #[test]
    fn write_resolved_is_atomic_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        write_resolved(dir.path(), &sample("repo-audit-fast")).unwrap();
        // No temp file stays behind.
        let entries: Vec<_> = std::fs::read_dir(dir.path().join("listings"))
            .unwrap()
            .collect();
        assert_eq!(entries.len(), 1, "a temp file was left behind");
        let read = read_resolved(dir.path(), "repo-audit-fast")
            .unwrap()
            .unwrap();
        assert_eq!(read, sample("repo-audit-fast"));
    }

    /// R2-7: the temp file and the directory are synced around the
    /// rename, so a power loss cannot leave an empty renamed file. The
    /// sync calls themselves are not observable; the proof is that the
    /// synced write still round trips from a re-opened directory.
    #[test]
    fn write_resolved_syncs_the_temp_file_and_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_resolved(dir.path(), &sample("repo-audit-fast")).unwrap();
        // The written file is complete and parses from a fresh read.
        let text =
            std::fs::read_to_string(dir.path().join("listings/repo-audit-fast.json")).unwrap();
        assert!(!text.is_empty(), "the renamed file is empty");
        let r: ResolvedListing = serde_json::from_str(&text).unwrap();
        assert_eq!(r, sample("repo-audit-fast"));
    }
}
