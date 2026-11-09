//! `nearbee list <listing.yaml>`: materialise the listing's
//! playbook into Nearbee's apb instance, patch the market agent, and set
//! the pricing plan. Publishing never changes `listing_status`: the owner
//! publishes a draft, and drafts stay drafts.

use std::io::Write;
use std::path::{Path, PathBuf};

use nearbee_apb::install::{self, ApbCli, Script};
use nearbee_core::credentials::Store;
use nearbee_core::listing::{self, Listing, ResolvedListing};
use nearbee_core::market::{AgentPatch, Market as _, MarketError, NewPricingPlan};
use nearbee_core::secret::Secret;
use nearbee_market::NearAgentMarket;

/// The settings `nearbee list` needs beyond the config file.
#[derive(Debug, Clone)]
pub struct PublishEnv {
    /// The market origin (MARKET_URL).
    pub market_url: String,
    /// The apb binary (APB_BIN or `apb`).
    pub apb_bin: PathBuf,
    /// The apb instance's config directory (APB_CONFIG_DIR).
    pub apb_config_dir: PathBuf,
    /// The apb project Nearbee's playbooks live in.
    pub apb_workspace: PathBuf,
}

/// A publish failure. Market refusals keep their machine code.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("{0}")]
    Other(String),
    /// The agent already holds the maximum number of pricing plans.
    #[error(
        "the market refused a new pricing plan (409 {code}): the agent holds the maximum number of pricing plans; remove an unused plan on the market and retry. No plan was deleted or patched."
    )]
    PlanCap { code: String },
}

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

/// The publish. `apb` runs the materialisation and install; the market
/// calls go through the agent's token from the credentials store.
pub async fn publish(
    listing_file: &Path,
    pub_env: &PublishEnv,
    store: &Store,
    state_dir: &Path,
    secrets: Vec<Secret>,
) -> Result<ResolvedListing, PublishError> {
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
    let source = source.clone();
    let installed = tokio::task::spawn_blocking(move || {
        install::install_source(&cli, &source, &handle, &scripts)
    })
    .await
    .map_err(|e| PublishError::Other(format!("install: {e}")))?
    .map_err(|e| PublishError::Other(e.to_string()))?;

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

    // The resolved record is written before the market calls (round 5,
    // codex 4). Chosen over rolling back the approval because it is the
    // simpler of the two: the record always names the version apb now
    // runs, so a market call that fails after a successful install can
    // never leave the record pointing at the previous (already replaced)
    // version while apb runs the new one. The owner re-runs `nearbee
    // list`; the record's `published_at` is when this install happened.
    let resolved = ResolvedListing {
        handle: l.handle.clone(),
        agent_id: creds.agent_id.clone(),
        playbook_id: installed.playbook_id.clone(),
        playbook_version: installed.version.clone(),
        playbook_sha256: installed.sha256.clone(),
        price_usd: l.price_usd.clone(),
        sla_seconds: l.sla_seconds,
        published_at: time::OffsetDateTime::now_utc(),
    };
    listing::write_resolved(state_dir, &resolved).map_err(PublishError::Other)?;

    // Market update: the pricing plan first, then the patch. A failure in
    // the plan step then leaves the agent text untouched, instead of a
    // visible intermediate state with a new text but the old price.
    let plans = market
        .pricing_plans(&creds.agent_id)
        .await
        .map_err(|e| map_market(e, &resolved.playbook_version))?;
    let default = plans.iter().find(|p| p.is_default);
    let same = default.is_some_and(|p| {
        p.model.as_deref() == Some("per_call")
            && same_price(&p.price_amount, &l.price_usd)
            && p.sla_seconds == Some(l.sla_seconds)
    });
    let _plan_id = if same {
        default
            .map(|p| p.pricing_plan_id.clone())
            .unwrap_or_default()
    } else {
        let created = market
            .create_pricing_plan(
                &creds.agent_id,
                &NewPricingPlan {
                    price_usd: l.price_usd.clone(),
                    sla_seconds: l.sla_seconds,
                    display_name: Some(format!("{} per call", l.handle)),
                },
            )
            .await
            .map_err(|e| match e {
                MarketError::Refused {
                    status: 409, code, ..
                } => PublishError::PlanCap { code },
                other => map_market(other, &resolved.playbook_version),
            })?;
        let id = created.pricing_plan_id.clone();
        market
            .make_default(&creds.agent_id, &id)
            .await
            .map_err(|e| map_market(e, &resolved.playbook_version))?;
        id
    };
    let patch = AgentPatch {
        name: Some(l.name.clone()),
        description: Some(l.description.clone()),
        bio_markdown: Some(l.bio_markdown.clone()),
        category: Some(l.category.clone()),
        tags: Some(l.tags.clone()),
        sla_seconds: Some(l.sla_seconds),
    };
    market
        .patch_agent(&creds.agent_id, &patch)
        .await
        .map_err(|e| map_market(e, &resolved.playbook_version))?;

    Ok(resolved)
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
    )
    .await
    {
        Ok(r) => {
            let _ = writeln!(
                out,
                "published {} ({}, {} USD, SLA {}s) on {} version {} digest {}",
                r.handle,
                r.agent_id,
                r.price_usd,
                r.sla_seconds,
                r.playbook_id,
                r.playbook_version,
                &r.playbook_sha256[..12.min(r.playbook_sha256.len())]
            );
            0
        }
        Err(e) if matches!(e, PublishError::PlanCap { .. }) => {
            let _ = writeln!(out, "error: {e}");
            1
        }
        Err(e) => {
            let _ = writeln!(out, "error: {e}");
            1
        }
    }
}
