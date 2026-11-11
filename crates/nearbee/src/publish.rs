//! `nearbee list <listing.yaml>`: materialise the listing's playbook
//! into Nearbee's apb instance, record the listing, read the pricing
//! plans and print what the owner sets in the market web app. Publishing
//! sends no market write: an agent token cannot change the agent (403
//! `agent_token_scope_forbidden`, measured 2026-11-10), so the owner sets
//! name, description, bio, category, tags, SLA and price in the web app
//! (D-024). Publishing never changes `listing_status`: the owner
//! publishes a draft, and drafts stay drafts.

use std::io::Write;
use std::path::{Path, PathBuf};

use nearbee_apb::install::{self, ApbCli, Script};
use nearbee_apb::{ApbOptions, ApbRunner, Timing};
use nearbee_core::catalog::Catalog;
use nearbee_core::credentials::Store;
use nearbee_core::journal::sha256_hex;
use nearbee_core::listing::{self, Listing, ResolvedListing, Tier};
use nearbee_core::market::{Market as _, MarketError};
use nearbee_core::runner::{PlaybookId, Runner};
use nearbee_core::secret::Secret;
use nearbee_core::statement;
use nearbee_core::tiers;
use nearbee_market::NearAgentMarket;

/// The settings `nearbee list` needs beyond the config file.
#[derive(Debug, Clone)]
pub struct PublishEnv {
    /// The market origin (MARKET_URL).
    pub market_url: String,
    /// The apb instance's REST origin (the config's `apb.url`): the
    /// inspection reads the facts from it.
    pub apb_url: String,
    /// The apb instance's API key (the config's `apb.api_key`).
    pub apb_api_key: Option<Secret>,
    /// The transport settings of the apb calls.
    pub timing: Timing,
    /// The apb binary (APB_BIN or `apb`).
    pub apb_bin: PathBuf,
    /// The apb instance's config directory (the config's `apb.config_dir`).
    pub apb_config_dir: PathBuf,
    /// The instance's expected `XDG_CONFIG_HOME` (the config's
    /// `apb.xdg_config_home`).
    pub apb_xdg_config_home: PathBuf,
    /// The `systemctl` binary the inspection reads the instance
    /// environment with.
    pub systemctl_bin: PathBuf,
    /// The apb project Nearbee's playbooks live in.
    pub apb_workspace: PathBuf,
}

/// A publish failure. Market refusals keep their machine code.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("{0}")]
    Other(String),
}

/// One row of the web-app settings report (design 13): what the file says,
/// what the market shows (a draft agent is not readable with the agent
/// token), and whether the value still has to be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRow {
    pub field: &'static str,
    pub file_value: String,
    pub market_value: String,
    /// `ok`, `set in web app`, `mismatch` or `unverified`.
    pub status: &'static str,
}

/// The market value shown when the token cannot read it.
const NOT_READABLE: &str = "not readable with the agent token";
/// The market value shown when no default plan exists yet.
const NOT_SET: &str = "not set in the web app yet";

/// Reads the source playbook and its scripts directory (sibling `scripts/`).
/// The source stays inside the listing file's directory tree: an absolute
/// path or a `..` escape is refused.
fn read_source(
    source_playbook: &str,
    listing_file: &Path,
) -> Result<(String, Vec<Script>), String> {
    let listing_dir = listing_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let source_path = if source_playbook.starts_with('/') {
        PathBuf::from(source_playbook)
    } else {
        listing_dir.join(source_playbook)
    };
    let canonical = source_path
        .canonicalize()
        .map_err(|e| format!("source playbook {}: {e}", source_path.display()))?;
    let listing_root = listing_dir
        .canonicalize()
        .unwrap_or_else(|_| listing_dir.clone());
    if !canonical.starts_with(&listing_root) {
        return Err(format!(
            "source playbook {} is outside the listing file's directory tree",
            source_path.display()
        ));
    }
    let text = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("source playbook {}: {e}", canonical.display()))?;
    let mut scripts = Vec::new();
    let dir = canonical.parent().unwrap_or(Path::new(".")).join("scripts");
    if dir.is_dir() {
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .into_iter()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        names.sort();
        for p in names {
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(&p)
                    .map(|m| m.permissions().mode())
                    .unwrap_or(0)
                    & 0o111
                    != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            scripts.push(Script {
                name: p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                bytes,
                executable,
            });
        }
    }
    Ok((text, scripts))
}

/// Runs the tier check on one set of facts; a breach stops the publish
/// with every breach named (spec `listing-publish`, Tier check at
/// publish).
fn check_tier(
    tier: Tier,
    facts: &nearbee_core::tiers::ExecutionFacts,
    catalog: &Catalog,
    when: &str,
) -> Result<(), PublishError> {
    if let Err(breaches) = tiers::check(tier, facts, catalog) {
        return Err(PublishError::Other(format!(
            "the {} listing breaks the tier {}: {}",
            tier.as_str(),
            when,
            breaches.join("; ")
        )));
    }
    Ok(())
}

/// The publish. `apb` runs the materialisation and install; the market
/// calls go through the agent's token from the credentials store.
pub async fn publish(
    listing_file: &Path,
    pub_env: &PublishEnv,
    store: &Store,
    state_dir: &Path,
    secrets: Vec<Secret>,
    retention_days: u32,
) -> Result<(ResolvedListing, Vec<SettingRow>), PublishError> {
    let text = std::fs::read_to_string(listing_file)
        .map_err(|e| PublishError::Other(format!("{}: {e}", listing_file.display())))?;
    let l: Listing =
        listing::parse(&text, listing_file).map_err(|e| PublishError::Other(e.to_string()))?;
    let creds = store
        .get(&l.handle)
        .map_err(|e| PublishError::Other(e.to_string()))?
        .ok_or_else(|| {
            PublishError::Other(format!(
                "no credentials for {}; run: nearbee agent add {}",
                l.handle, l.handle
            ))
        })?;
    let catalog: &Catalog = Catalog::embedded().map_err(|e| PublishError::Other(e.to_string()))?;
    // The inspection reads the facts from Nearbee's own apb instance: its
    // REST routes for the playbook and profiles, its workspace files for
    // the routes and decisions, its health route for the version.
    let runner = ApbRunner::connect(
        ApbOptions {
            url: pub_env.apb_url.clone(),
            api_key: pub_env.apb_api_key.clone(),
            redact: secrets.clone(),
            timing: pub_env.timing.clone(),
            bin: pub_env.apb_bin.clone(),
            config_dir: pub_env.apb_config_dir.clone(),
            systemctl_bin: pub_env.systemctl_bin.clone(),
            xdg_config_home: pub_env.apb_xdg_config_home.clone(),
        },
        &pub_env.apb_workspace,
    )
    .await
    .map_err(|e| PublishError::Other(format!("connect to the apb instance: {e}")))?;
    // Materialise and install first: no market call before the playbook is
    // in the apb instance with trial_candidates off.
    let (source, scripts) =
        read_source(&l.source_playbook, listing_file).map_err(PublishError::Other)?;
    // The apb install is a blocking subprocess (its pipes are drained on
    // threads), so it runs on a blocking thread and never stalls the async
    // runtime (ocr2 14).
    let cli = std::sync::Arc::new(ApbCli {
        bin: pub_env.apb_bin.clone(),
        workspace: pub_env.apb_workspace.clone(),
        config_dir: pub_env.apb_config_dir.clone(),
    });
    let handle = l.handle.clone();
    let tier = l.tier;
    // The listing's profile is the variant: two listings on one source
    // install two playbooks with different defaults.profile values.
    let profile = l.profile.clone();
    // The publish order (design 4.4): the materialised source is checked
    // before anything is installed, so a tier breach never reaches apb.
    let materialised = install::materialise(&source, &handle, profile.as_deref(), &scripts)
        .map_err(|e| PublishError::Other(e.to_string()))?;
    let source_facts = runner
        .inspect_source(&materialised)
        .await
        .map_err(|e| PublishError::Other(format!("the source facts cannot be read: {e}")))?
        .with_direct_models(&l.direct_models, &l.signed_models);
    check_tier(tier, &source_facts, catalog, "before installing")?;
    let materialised_for_task = materialised.clone();
    let scripts_for_task = scripts.clone();
    let installed = tokio::task::spawn_blocking(move || {
        install::install_playbook(&cli, &materialised_for_task, &scripts_for_task)
    })
    .await
    .map_err(|e| PublishError::Other(format!("install: {e}")))?
    .map_err(|e| PublishError::Other(e.to_string()))?;

    // The installed version is read back from apb and checked again: the
    // installed facts must pass the same tier and match the source's
    // fingerprint (design 4.4).
    let installed_facts = runner
        .inspect(&PlaybookId::new(handle.clone()))
        .await
        .map_err(|e| PublishError::Other(format!("the installed facts cannot be read: {e}")))?
        .with_direct_models(&l.direct_models, &l.signed_models);
    check_tier(tier, &installed_facts, catalog, "after installing")?;
    let source_fingerprint = statement::fingerprint(tier, &source_facts);
    let fingerprint = statement::fingerprint(tier, &installed_facts);
    if source_fingerprint != fingerprint {
        return Err(PublishError::Other(
            "installed facts differ from the source; nothing was recorded: run nearbee list again"
                .into(),
        ));
    }
    let stmt = statement::render(tier, &installed_facts, retention_days);
    let statement_sha256 = sha256_hex(stmt.as_bytes());
    let evidence_models = tiers::evidence_models(&installed_facts);

    // Market update: patch, then the pricing plan. The token travels only
    // as a bearer header to the market origin.
    let mut redact = secrets;
    redact.push(creds.token.clone());
    let market = NearAgentMarket::new(nearbee_market::MarketOptions {
        base_url: pub_env.market_url.clone(),
        token: creds.token.clone(),
        redact,
        timing: Default::default(),
    })
    .map_err(|e| PublishError::Other(e.to_string()))?;

    // The resolved record is written before the market read (round 5,
    // codex 4): it always names the version apb now runs, so a read that
    // fails after a successful install can never leave the record
    // pointing at the previous version while apb runs the new one.
    let resolved = ResolvedListing {
        handle: l.handle.clone(),
        agent_id: creds.agent_id.clone(),
        playbook_id: installed.playbook_id.clone(),
        playbook_version: installed.version.clone(),
        playbook_sha256: installed.sha256.clone(),
        price_usd: l.price_usd.clone(),
        sla_seconds: l.sla_seconds,
        // The scope falls back to the description here, so serve reads one
        // value (an S2 record without either skips the scope check).
        scope: Some(l.scope.clone().unwrap_or_else(|| l.description.clone())),
        protection: Some(l.protection),
        limits: l.limits.clone(),
        tier: Some(tier),
        intake_check: l.intake_check,
        statement: Some(stmt),
        statement_sha256: Some(statement_sha256),
        statement_format: Some(statement::STATEMENT_FORMAT),
        fingerprint: Some(fingerprint),
        evidence_models,
        direct_models: l.direct_models.clone(),
        signed_models: l.signed_models.clone(),
        published_at: time::OffsetDateTime::now_utc(),
    };
    listing::write_resolved(state_dir, &resolved).map_err(PublishError::Other)?;

    // The one market call left: the pricing plans read (design 13). The
    // agent itself is not read: a draft answers 404 with the agent token
    // (measured 2026-11-10).
    let plans = market
        .pricing_plans(&creds.agent_id)
        .await
        .map_err(|e| map_market(e, &resolved.playbook_version))?;
    let default = plans.iter().find(|p| p.is_default);
    let price_ok = default.is_some_and(|p| {
        p.model.as_deref() == Some("per_call") && same_price(&p.price_amount, &l.price_usd)
    });
    let mut rows = Vec::new();
    for (field, value) in [
        ("name", l.name.clone()),
        ("description", l.description.clone()),
        ("bio", l.bio_markdown.clone()),
        ("category", l.category.clone()),
        ("tags", l.tags.join(", ")),
    ] {
        rows.push(SettingRow {
            field,
            file_value: value,
            market_value: NOT_READABLE.into(),
            status: "set in web app",
        });
    }
    let (sla_market, sla_status) = match default {
        Some(p) => match p.sla_seconds {
            Some(s) if s == l.sla_seconds => (s.to_string(), "ok"),
            Some(s) => (s.to_string(), "mismatch"),
            // A null plan SLA inherits the agent default, which the token
            // cannot read.
            None => ("inherits the agent default".into(), "unverified"),
        },
        None => (NOT_SET.into(), "set in web app"),
    };
    rows.push(SettingRow {
        field: "SLA",
        file_value: format!("{}s", l.sla_seconds),
        market_value: sla_market,
        status: sla_status,
    });
    let (price_market, price_status) = match default {
        Some(p) if price_ok => (p.price_amount.clone(), "ok"),
        Some(p) => (p.price_amount.clone(), "mismatch"),
        None => (NOT_SET.into(), "set in web app"),
    };
    rows.push(SettingRow {
        field: "price",
        file_value: format!("{} USD", l.price_usd),
        market_value: price_market,
        status: price_status,
    });

    Ok((resolved, rows))
}

/// Two price strings are the same amount.
fn same_price(a: &str, b: &str) -> bool {
    match (
        a.parse::<rust_decimal::Decimal>(),
        b.parse::<rust_decimal::Decimal>(),
    ) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// A market failure after the install: the record already names the
/// installed version (so serve runs exactly what apb runs), and the
/// operator finishes the market update by publishing again.
fn map_market(e: MarketError, version: &str) -> PublishError {
    PublishError::Other(format!(
        "{e}; playbook version {version} is installed and recorded for this listing, but the market update is incomplete: run nearbee list again"
    ))
}

/// The CLI wrapper: loads the config, runs the publish, prints the result.
pub async fn publish_list(
    cfg: &nearbee_core::config::Config,
    listing_file: &Path,
    pub_env: &PublishEnv,
    store: &Store,
    secrets: Vec<Secret>,
    out: &mut dyn Write,
) -> i32 {
    match publish(
        listing_file,
        pub_env,
        store,
        cfg.state_dir.as_path(),
        secrets,
        cfg.retention.after_acceptance_days,
    )
    .await
    {
        Ok((r, rows)) => {
            let _ = writeln!(
                out,
                "installed {} ({}, {} USD, SLA {}s) on {} version {} digest {}",
                r.handle,
                r.agent_id,
                r.price_usd,
                r.sla_seconds,
                r.playbook_id,
                r.playbook_version,
                &r.playbook_sha256[..12.min(r.playbook_sha256.len())]
            );
            if let Some(tier) = r.tier {
                let _ = writeln!(out, "tier: {}", tier.as_str());
            }
            // The statement is what the owner pastes at the end of the
            // bio in the web app (spec `listing-publish`, Statement in
            // the settings report).
            if let Some(stmt) = &r.statement {
                let _ = writeln!(out);
                let _ = writeln!(
                    out,
                    "paste this block at the end of the bio in the web app:"
                );
                let _ = writeln!(out, "{stmt}");
            }
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "set these in the market web app (https://market.near.ai):"
            );
            let width = rows.iter().map(|r| r.field.len()).max().unwrap_or(0);
            for r in &rows {
                let _ = writeln!(
                    out,
                    "  {:<width$}  {}  |  {}  |  {}",
                    r.field,
                    r.file_value,
                    r.market_value,
                    r.status,
                    width = width
                );
            }
            0
        }
        Err(e) => {
            let _ = writeln!(out, "error: {e}");
            1
        }
    }
}
