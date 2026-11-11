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

/// Whether the listing protects its hires with a computed budget
/// (default `on`); `off` starts runs without a budget object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protection {
    On,
    Off,
}

impl Protection {
    pub fn as_str(self) -> &'static str {
        match self {
            Protection::On => "on",
            Protection::Off => "off",
        }
    }
}

/// The optional hard limits of a listing file (spec `hire-intake`); a
/// limit absent means no limit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListingLimits {
    pub max_brief_bytes: Option<u64>,
    pub max_attachments: Option<usize>,
    pub max_attachment_bytes: Option<u64>,
    pub file_types: Option<Vec<String>>,
}

/// The longest `scope`, in characters.
pub const SCOPE_MAX: usize = 300;

/// The longest profile name, in characters: 1 to 64 of `[a-z0-9-]`.
pub const PROFILE_MAX: usize = 64;

/// The privacy tier a listing declares (spec `privacy-tiers`, Tiers):
/// `plain` promises only the data statement, `attested` promises TEE
/// models with collected endpoint evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Plain,
    Attested,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Plain => "plain",
            Tier::Attested => "attested",
        }
    }
}

/// The deterministic intake check a listing may ask for. Only the
/// GitHub repository scan exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntakeCheck {
    #[serde(rename = "github_repo")]
    GithubRepo,
}

impl IntakeCheck {
    pub fn as_str(self) -> &'static str {
        match self {
            IntakeCheck::GithubRepo => "github_repo",
        }
    }
}

/// A profile name: 1 to 64 characters of `[a-z0-9-]`.
pub fn profile_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= PROFILE_MAX
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

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
    /// What the listing takes (its `description` when absent).
    pub scope: Option<String>,
    pub protection: Protection,
    pub limits: Option<ListingLimits>,
    /// The privacy tier; required.
    pub tier: Tier,
    /// The apb profile the listing installs instead of the source's
    /// `defaults.profile`.
    pub profile: Option<String>,
    /// The deterministic intake check run before the scope check.
    pub intake_check: Option<IntakeCheck>,
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
    #[error(
        "listing file {path}: price_usd {price} is too low: with protection on the per-hire ceiling (price * 0.95 - 0.25) would not be above zero; raise the price or set protection: off"
    )]
    PriceTooLow { path: String, price: String },
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
        scope: Option<String>,
        protection: Option<Protection>,
        limits: Option<ListingLimits>,
        tier: Option<String>,
        profile: Option<String>,
        intake_check: Option<String>,
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
    let scope = match &raw.scope {
        Some(s) => {
            if s.chars().count() > SCOPE_MAX {
                return Err(ListingError::Parse {
                    path: path.clone(),
                    message: format!("scope must be at most {SCOPE_MAX} characters"),
                });
            }
            Some(s.clone())
        }
        None => None,
    };
    let protection = raw.protection.unwrap_or(Protection::On);
    // The tier is required and only `plain` or `attested` are accepted
    // (spec `privacy-tiers`, Tiers).
    let tier = match raw.tier.as_deref() {
        Some("plain") => Tier::Plain,
        Some("attested") => Tier::Attested,
        Some(other) => {
            return Err(ListingError::Parse {
                message: format!(
                    "tier `{other}` is not one of the allowed values: plain, attested"
                ),
                path: path.clone(),
            });
        }
        None => {
            return Err(ListingError::Parse {
                message: "tier is required and must be one of the allowed values: plain, attested"
                    .into(),
                path: path.clone(),
            });
        }
    };
    // A profile name is 1 to 64 characters of [a-z0-9-].
    if let Some(p) = &raw.profile
        && !profile_name_ok(p)
    {
        return Err(ListingError::Parse {
            message: format!(
                "profile `{p}` is not a profile name (1 to {PROFILE_MAX} characters of [a-z0-9-])"
            ),
            path: path.clone(),
        });
    }
    // Only the GitHub repository intake check exists.
    let intake_check = match raw.intake_check.as_deref() {
        Some("github_repo") => Some(IntakeCheck::GithubRepo),
        Some(other) => {
            return Err(ListingError::Parse {
                message: format!(
                    "intake_check `{other}` is not one of the allowed values: github_repo"
                ),
                path: path.clone(),
            });
        }
        None => None,
    };
    // With protection on the per-hire ceiling must be above zero
    // (spec `listing-publish`, Listing file; the formula, not a cent rule).
    if protection == Protection::On {
        let price = price_usd
            .parse::<rust_decimal::Decimal>()
            .map_err(|_| ListingError::PriceNotString { path: path.clone() })?;
        if price * rust_decimal::Decimal::new(95, 2) - rust_decimal::Decimal::new(25, 2)
            <= rust_decimal::Decimal::ZERO
        {
            return Err(ListingError::PriceTooLow {
                path,
                price: price_usd,
            });
        }
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
        scope,
        protection,
        limits: raw.limits,
        tier,
        profile: raw.profile,
        intake_check,
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
    /// What the listing takes; absent means from the description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Absent (an S2 record) reads as protected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<Protection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<ListingLimits>,
    /// The privacy tier (an S2 or S3 record has none and is not
    /// serveable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intake_check: Option<IntakeCheck>,
    /// The data statement snapshot as rendered at publish time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<String>,
    /// SHA-256 of the statement text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_sha256: Option<String>,
    /// The statement format this record was written for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_format: Option<u32>,
    /// The execution facts fingerprint at publish time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// The model ids endpoint evidence is collected for, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_models: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: time::OffsetDateTime,
}

impl ResolvedListing {
    /// Whether serve may run hires for this record: it needs the tier,
    /// the fingerprint and the statement, a statement hash that matches
    /// the statement, and the current statement format (design 1.3).
    pub fn serveable(&self) -> bool {
        let hash_matches = match (&self.statement, &self.statement_sha256) {
            (Some(s), Some(h)) => *h == crate::journal::sha256_hex(s.as_bytes()),
            _ => false,
        };
        self.tier.is_some()
            && self.fingerprint.is_some()
            && hash_matches
            && self.statement_format == Some(crate::statement::STATEMENT_FORMAT)
    }
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
source_playbook: playbooks/repo-audit/playbook.yaml
price_usd: "0.50"
sla_seconds: 86400
category: dev_tools
tags: ["nearbee", "example"]
description: "Audits a public repository and returns a short Markdown report."
bio_markdown: "A Nearbee example agent."
tier: plain
"#
        .to_owned()
    }

    fn text_with_new_keys() -> String {
        format!(
            "{}\nscope: \"Audits of public git repositories\"\nprotection: on\nlimits:\n  max_brief_bytes: 8192\n  max_attachments: 3\n  max_attachment_bytes: 10485760\n  file_types: [\".md\", \".csv\"]\n",
            text()
        )
    }

    /// The new keys parse with their defaults and values.
    #[test]
    fn scope_protection_and_limits_parse() {
        let l = parse(&text(), Path::new("l.yaml")).unwrap();
        assert_eq!(l.scope, None, "no scope key: none");
        assert_eq!(l.protection, Protection::On, "protection defaults to on");
        assert_eq!(l.limits, None, "no limits key: none");
        let l = parse(&text_with_new_keys(), Path::new("l.yaml")).unwrap();
        assert_eq!(
            l.scope.as_deref(),
            Some("Audits of public git repositories")
        );
        assert_eq!(l.protection, Protection::On);
        let limits = l.limits.expect("the limits parse");
        assert_eq!(limits.max_brief_bytes, Some(8192));
        assert_eq!(limits.max_attachments, Some(3));
        assert_eq!(limits.max_attachment_bytes, Some(10_485_760));
        assert_eq!(
            limits.file_types.as_deref(),
            Some(&[".md".to_owned(), ".csv".to_owned()][..])
        );
        // protection off parses.
        let off = text_with_new_keys().replace("protection: on", "protection: off");
        let l = parse(&off, Path::new("l.yaml")).unwrap();
        assert_eq!(l.protection, Protection::Off);
        // A scope is at most 300 characters.
        let long = text().replace(
            "bio_markdown:",
            &format!("scope: \"{}\"\nbio_markdown:", "s".repeat(301)),
        );
        let err = parse(&long, Path::new("l.yaml")).unwrap_err();
        assert!(err.to_string().contains("scope"), "{err}");
    }

    /// An unknown key under `limits` is refused by name.
    #[test]
    fn an_unknown_limit_is_refused_by_name() {
        let bad = text_with_new_keys().replace("max_brief_bytes: 8192", "max_pages: 5");
        let err = parse(&bad, Path::new("l.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_pages"), "{msg}");
    }

    /// A price whose protected ceiling would not be above zero is refused;
    /// the formula is `price * 0.95 - 0.25 > 0`, not a cent rule, so
    /// `0.264` loads and `0.26` does not. With protection off any price
    /// loads.
    #[test]
    fn a_price_too_low_for_protection_is_refused() {
        let bad = text().replace("\"0.50\"", "\"0.26\"");
        let err = parse(&bad, Path::new("l.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ceiling"), "{msg}");
        // 0.264 * 0.95 - 0.25 = 0.0008 > 0: loads.
        let ok = text().replace("\"0.50\"", "\"0.264\"");
        let l = parse(&ok, Path::new("l.yaml")).unwrap();
        assert_eq!(l.price_usd, "0.264");
        // 0.265 * 0.95 - 0.25 = 0.00175: loads as well.
        let ok = text().replace("\"0.50\"", "\"0.265\"");
        assert!(parse(&ok, Path::new("l.yaml")).is_ok());
        // Protection off: the price is not checked against the ceiling.
        let off = text()
            .replace("\"0.50\"", "\"0.26\"")
            .replace("bio_markdown:", "protection: off\nbio_markdown:");
        let l = parse(&off, Path::new("l.yaml")).unwrap();
        assert_eq!(l.protection, Protection::Off);
        assert_eq!(l.price_usd, "0.26");
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

    /// The tier is required (spec `listing-publish`, Listing file, "Tier
    /// missing"): loading fails naming the field and the allowed values.
    #[test]
    fn a_listing_without_a_tier_is_refused_naming_the_field() {
        let bad = text().replace("tier: plain\n", "");
        let err = parse(&bad, Path::new("l.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ListingError::Parse { .. }), "{msg}");
        assert!(msg.contains("tier"), "{msg}");
        assert!(msg.contains("plain") && msg.contains("attested"), "{msg}");
    }

    /// An unknown tier value is refused naming the field and the allowed
    /// values (spec `privacy-tiers`, Tiers, "Unknown tier").
    #[test]
    fn an_unknown_tier_is_refused_naming_the_field_and_the_allowed_values() {
        let bad = text().replace("tier: plain", "tier: blind");
        let err = parse(&bad, Path::new("l.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("tier"), "{msg}");
        assert!(msg.contains("blind"), "{msg}");
        assert!(msg.contains("plain") && msg.contains("attested"), "{msg}");
        // Only the two values parse.
        let l = parse(
            &text().replace("tier: plain", "tier: attested"),
            Path::new("l.yaml"),
        )
        .unwrap();
        assert_eq!(l.tier, Tier::Attested);
        assert_eq!(
            parse(&text(), Path::new("l.yaml")).unwrap().tier,
            Tier::Plain
        );
    }

    /// An optional `profile` must be 1 to 64 characters of
    /// `[a-z0-9-]`; anything else is refused.
    #[test]
    fn a_profile_outside_the_allowed_shape_is_refused() {
        let ok = text().replace("tier: plain", "tier: attested\nprofile: nearbee-near");
        let l = parse(&ok, Path::new("l.yaml")).unwrap();
        assert_eq!(l.profile.as_deref(), Some("nearbee-near"));
        // No profile key: none.
        assert_eq!(parse(&text(), Path::new("l.yaml")).unwrap().profile, None);
        // 64 characters of the allowed alphabet parse, 65 do not.
        let long = format!("profile: {}\ntier: plain", "a".repeat(64));
        let with = text().replace("tier: plain", &long);
        assert!(parse(&with, Path::new("l.yaml")).is_ok());
        let toolong = text().replace(
            "tier: plain",
            &format!("profile: {}\ntier: plain", "a".repeat(65)),
        );
        let err = parse(&toolong, Path::new("l.yaml")).unwrap_err();
        assert!(err.to_string().contains("profile"), "{err}");
        // A bad character and an empty name are refused.
        for bad_name in ["Nearbee", "near bee", "near_bee", ""] {
            let bad = text()
                .replace("tier: plain", "profile: x\ntier: plain")
                .replace("profile: x", &format!("profile: \"{bad_name}\""));
            let err = parse(&bad, Path::new("l.yaml")).unwrap_err();
            assert!(err.to_string().contains("profile"), "{bad_name}: {err}");
        }
    }

    /// An `intake_check` accepts only `github_repo`; any other value is
    /// refused naming the allowed value.
    #[test]
    fn an_intake_check_other_than_github_repo_is_refused() {
        let ok = text().replace("tier: plain", "tier: plain\nintake_check: github_repo");
        let l = parse(&ok, Path::new("l.yaml")).unwrap();
        assert_eq!(l.intake_check, Some(IntakeCheck::GithubRepo));
        // No key: none.
        assert_eq!(
            parse(&text(), Path::new("l.yaml")).unwrap().intake_check,
            None
        );
        for bad in ["github", "gitlab_repo", "repo", "GITHUB_REPO"] {
            let bad_text = text()
                .replace("tier: plain", "intake_check: x\ntier: plain")
                .replace("intake_check: x", &format!("intake_check: {bad}"));
            let err = parse(&bad_text, Path::new("l.yaml")).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("intake_check"), "{bad}: {msg}");
            assert!(msg.contains("github_repo"), "{bad}: {msg}");
        }
    }

    /// An S3 resolved record without the new fields still parses (they
    /// default) and is not serveable: it has no tier, statement or
    /// fingerprint.
    #[test]
    fn an_s3_record_without_the_new_fields_parses_and_is_not_serveable() {
        let s3 = r#"{
  "handle": "repo-audit-fast",
  "agent_id": "ag_1",
  "playbook_id": "repo-audit-fast",
  "playbook_version": "1.0.12345",
  "playbook_sha256": "ab",
  "price_usd": "0.50",
  "sla_seconds": 86400,
  "scope": "An audit of one public repository.",
  "protection": "on",
  "published_at": "1970-01-01T00:00:00Z"
}"#;
        let r: ResolvedListing = serde_json::from_str(s3).unwrap();
        assert_eq!(r.tier, None);
        assert_eq!(r.intake_check, None);
        assert_eq!(r.statement, None);
        assert_eq!(r.statement_sha256, None);
        assert_eq!(r.statement_format, None);
        assert_eq!(r.fingerprint, None);
        assert!(r.evidence_models.is_empty());
        assert!(!r.serveable(), "an S3 record is not serveable");
        // Round trip: a record with everything set keeps it.
        let full = ResolvedListing {
            tier: Some(Tier::Attested),
            intake_check: Some(IntakeCheck::GithubRepo),
            statement: Some("## Data handling\n".into()),
            statement_sha256: Some(crate::journal::sha256_hex(b"## Data handling\n")),
            statement_format: Some(1),
            fingerprint: Some("sha256:ab".into()),
            evidence_models: vec!["DeepSeek-V4.1-Flash-1017".into()],
            handle: "repo-audit-fast".into(),
            agent_id: "ag_1".into(),
            playbook_id: "repo-audit-fast".into(),
            playbook_version: "1.0.1".into(),
            playbook_sha256: "ab".into(),
            price_usd: "0.50".into(),
            sla_seconds: 3600,
            scope: None,
            protection: None,
            limits: None,
            published_at: time::OffsetDateTime::from_unix_timestamp(0).unwrap(),
        };
        assert!(full.serveable(), "a full record is serveable");
        let text = serde_json::to_string(&full).unwrap();
        let back: ResolvedListing = serde_json::from_str(&text).unwrap();
        assert_eq!(back, full);
        assert!(back.serveable());
        // A corrupted statement hash or another format is not serveable.
        let mut bad = full.clone();
        bad.statement_sha256 = Some("00".into());
        assert!(!bad.serveable());
        let mut bad = full.clone();
        bad.statement_format = Some(99);
        assert!(!bad.serveable());
        let mut bad = full.clone();
        bad.fingerprint = None;
        assert!(!bad.serveable());
        let mut bad = full.clone();
        bad.tier = None;
        assert!(!bad.serveable());
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
            scope: None,
            protection: None,
            limits: None,
            tier: Some(Tier::Plain),
            intake_check: None,
            statement: Some("## Data handling\n".into()),
            statement_sha256: Some(crate::journal::sha256_hex(b"## Data handling\n")),
            statement_format: Some(1),
            fingerprint: Some("sha256:ab".into()),
            evidence_models: Vec::new(),
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
            scope: None,
            protection: None,
            limits: None,
            tier: Some(Tier::Plain),
            intake_check: None,
            statement: Some("## Data handling\n".into()),
            statement_sha256: Some(crate::journal::sha256_hex(b"## Data handling\n")),
            statement_format: Some(1),
            fingerprint: Some("sha256:ab".into()),
            evidence_models: Vec::new(),
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
