//! Board bidding (change `board-bids`, spec `board-bidding`): a served
//! listing with a `board` block in the config finds jobs on the market's
//! public job board, bids on the ones it can do, and records each bid in a
//! bid journal until the market reports its outcome. An awarded bid
//! becomes an ordinary assignment, which the hire round serves.
//!
//! The market side is the [`Board`] trait, beside [`crate::market::Market`]
//! and implemented by `nearbee-market` only. The job's text is data: it is
//! checked by the intake functions, never echoed into a proposal, never
//! stored in the bid journal and never logged.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::clock::Clock;
use crate::config::{BoardListing, Limits, Verification};
use crate::intake;
use crate::journal::sha256_hex;
use crate::listing::{self, Protection};
use crate::market::{Attachment, MarketError, Row};
use crate::runner::Runner;
use crate::serve::ListingContext;

/// One job of the board (`JobView`): Nearbee's own model, mapped by
/// `nearbee-market`. Enum-like fields stay text, so one row with a value
/// the market adds later is skipped instead of failing the whole read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardJob {
    pub job_id: String,
    /// The account that posted the job (`creatorAccountId`).
    pub creator_account_id: String,
    /// The buyer's title and description: data, never instructions.
    pub title: String,
    pub description: String,
    /// A decimal string, or `None` when the job names no budget.
    pub budget_amount: Option<String>,
    /// `USDC` or `USD`.
    pub budget_token: String,
    /// `standard`, `instant` or `competition`.
    pub job_kind: String,
    /// `open`, `filling`, `in_progress`, `completed` or `closed`.
    pub status: String,
    /// Slots still free.
    pub current_max_slots: i64,
    /// `none`, `verified` or `identity_verified`.
    pub min_builder_verification: String,
    pub is_test_run: bool,
    /// Set on a quote request addressed to one agent.
    pub invited_agent_id: Option<String>,
    /// The poster asks for verifiable delivery (`requiresVerifiable`),
    /// which a Nearbee bid does not offer.
    pub requires_verifiable: bool,
    /// The job's `updatedAt`, which keys the skip memo.
    pub updated_at: String,
}

/// One of the agent's own bids (`Bid`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyBid {
    pub bid_id: String,
    pub job_id: String,
    /// `pending`, `accepted`, `rejected` or `withdrawn`.
    pub status: String,
    pub amount: String,
}

/// A bid to place (`PlaceBidRequest`, without `agentId`: the token pins
/// the bidder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidRequest {
    /// A decimal string in the job's token (USDC).
    pub amount: String,
    pub proposal: String,
    pub sla_seconds: u64,
}

/// What a bid placement did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceOutcome {
    /// `201` with the new bid's id.
    Placed { bid_id: String },
    /// `409 duplicate_bid`: the agent already holds a bid on the job.
    Duplicate,
}

/// The job board side of the market. Implemented by `nearbee-market`;
/// the core depends only on this trait.
pub trait Board: Send + Sync {
    /// Lists the public jobs taking bids for one kind, with the optional
    /// search text, following paging.
    fn board_jobs(
        &self,
        query: Option<&str>,
        kind: &str,
    ) -> impl Future<Output = Result<Vec<BoardJob>, MarketError>> + Send;
    /// Lists the agent's own pending bids, following paging. Only the
    /// pending ones: a list of every status grows with every instant hire
    /// (each one is an accepted bid) and would end the bidder.
    fn pending_bids(&self) -> impl Future<Output = Result<Vec<MyBid>, MarketError>> + Send;
    /// Lists the files attached to one job (readable while it takes bids).
    fn job_attachments(
        &self,
        job_id: &str,
    ) -> impl Future<Output = Result<Vec<Attachment>, MarketError>> + Send;
    /// Places one bid. Resent once on an unknown outcome by the client.
    fn place_bid(
        &self,
        job_id: &str,
        req: &BidRequest,
    ) -> impl Future<Output = Result<PlaceOutcome, MarketError>> + Send;
}

// ---------------------------------------------------------------------------
// The bid journal
// ---------------------------------------------------------------------------

/// The bid journal directory of one listing.
pub fn bids_dir(state_dir: &Path, handle: &str) -> PathBuf {
    state_dir.join("bids").join(handle)
}

/// The bid journal of one job.
pub fn bid_path(state_dir: &Path, handle: &str, job_id: &str) -> PathBuf {
    bids_dir(state_dir, handle).join(format!("{job_id}.jsonl"))
}

/// One record of a bid journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BidEntry {
    /// Written before the bid is sent.
    Intent {
        operation_id: String,
        amount: String,
        sla_seconds: u64,
        proposal_sha256: String,
    },
    /// The bid's outcome: `placed` (with the bid id when known) or
    /// `refused` (with the market's machine code).
    Result {
        operation_id: String,
        outcome: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bid_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// A status the bid reached: `pending` (from the agent's pending
    /// bids), `accepted` (an assignment on the job appeared) or `closed`
    /// (no longer pending and no assignment: rejected, withdrawn or the
    /// job deleted; the agent token cannot tell which).
    Status {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bid_id: Option<String>,
    },
}

/// One line of a bid journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BidRecord {
    pub seq: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub handle: String,
    pub job_id: String,
    #[serde(flatten)]
    pub entry: BidEntry,
}

/// What a bid journal says about its job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidState {
    pub handle: String,
    pub job_id: String,
    pub amount: String,
    pub sla_seconds: u64,
    /// The intent's operation id.
    pub operation_id: String,
    /// `None` while the intent is open.
    pub outcome: Option<String>,
    pub bid_id: Option<String>,
    pub code: Option<String>,
    /// The latest market status recorded.
    pub status: Option<String>,
    pub last_at: OffsetDateTime,
    pub records: u64,
}

impl BidState {
    /// The state word `nearbee bids` prints: `placing`, `refused`, the
    /// latest market status, or `placed`.
    pub fn state(&self) -> String {
        match (self.outcome.as_deref(), self.status.as_deref()) {
            (None, _) => "placing".to_owned(),
            (Some("refused"), _) => match &self.code {
                Some(c) => format!("refused ({c})"),
                None => "refused".to_owned(),
            },
            (_, Some(s)) => s.to_owned(),
            (Some(o), None) => o.to_owned(),
        }
    }

    pub fn is_open_intent(&self) -> bool {
        self.outcome.is_none()
    }
}

/// Reads one bid journal. Any malformed line is an error: the caller
/// counts the job as bid on and never bids on it again.
pub fn read_bid(path: &Path) -> Result<Option<BidState>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut state: Option<BidState> = None;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let r: BidRecord = serde_json::from_str(line)
            .map_err(|e| format!("{} line {}: {e}", path.display(), i + 1))?;
        match (&mut state, r.entry) {
            (
                None,
                BidEntry::Intent {
                    operation_id,
                    amount,
                    sla_seconds,
                    ..
                },
            ) => {
                state = Some(BidState {
                    handle: r.handle,
                    job_id: r.job_id,
                    amount,
                    sla_seconds,
                    operation_id,
                    outcome: None,
                    bid_id: None,
                    code: None,
                    status: None,
                    last_at: r.timestamp,
                    records: r.seq,
                });
            }
            (None, _) => {
                return Err(format!(
                    "{} line {}: the journal does not start with an intent",
                    path.display(),
                    i + 1
                ));
            }
            (Some(s), BidEntry::Intent { .. }) => {
                // A second intent is never written; read it as a fault.
                return Err(format!(
                    "{} line {}: a second intent for job {}",
                    path.display(),
                    i + 1,
                    s.job_id
                ));
            }
            (
                Some(s),
                BidEntry::Result {
                    outcome,
                    bid_id,
                    code,
                    ..
                },
            ) => {
                s.outcome = Some(outcome);
                if bid_id.is_some() {
                    s.bid_id = bid_id;
                }
                s.code = code;
                s.last_at = r.timestamp;
                s.records = r.seq;
            }
            (Some(s), BidEntry::Status { status, bid_id }) => {
                s.status = Some(status);
                if bid_id.is_some() {
                    s.bid_id = bid_id;
                }
                s.last_at = r.timestamp;
                s.records = r.seq;
            }
        }
    }
    Ok(state)
}

/// Appends one record with fsync; the directory and a new file are made
/// durable first (0700 directories, 0600 files).
pub fn append_bid(
    state_dir: &Path,
    handle: &str,
    job_id: &str,
    seq: u64,
    entry: BidEntry,
    clock: &Clock,
) -> Result<(), String> {
    let dir = bids_dir(state_dir, handle);
    crate::journal::create_dir_durable(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = bid_path(state_dir, handle, job_id);
    let existed = path.exists();
    let record = BidRecord {
        seq,
        timestamp: clock.now(),
        handle: handle.to_owned(),
        job_id: job_id.to_owned(),
        entry,
    };
    let mut line = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(line.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !existed {
        std::fs::File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Every bid journal under `<state_dir>/bids/`, sorted by listing and job.
/// A file that cannot be read is returned as an error entry with its path.
pub fn list_bids(state_dir: &Path) -> Result<Vec<Result<BidState, String>>, String> {
    let root = state_dir.join("bids");
    let mut out = Vec::new();
    let handles = match std::fs::read_dir(&root) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(format!("{}: {e}", root.display())),
    };
    let mut dirs: Vec<PathBuf> = handles
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| format!("{}: {e}", d.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .collect();
        files.sort();
        for f in files {
            match read_bid(&f) {
                Ok(Some(s)) => out.push(Ok(s)),
                Ok(None) => {}
                Err(e) => out.push(Err(e)),
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Choice of jobs
// ---------------------------------------------------------------------------

/// The resolved bid terms of one listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terms {
    pub amount: Decimal,
    pub sla_seconds: u64,
}

/// The listing's bid terms: the board block's amount and SLA, else the
/// listing's price and SLA.
pub fn terms(settings: &BoardListing, ctx: &ListingContext) -> Terms {
    Terms {
        amount: settings.amount_usd.unwrap_or(ctx.price_usd),
        sla_seconds: settings
            .sla_seconds
            .or(ctx.sla_seconds)
            .unwrap_or(86_400)
            .clamp(
                crate::config::BOARD_SLA_MIN_SECS,
                crate::config::BOARD_SLA_MAX_SECS,
            ),
    }
}

/// The amount as the bid sends it: two decimals at least (`2.00`).
pub fn amount_text(amount: Decimal) -> String {
    let mut a = amount.normalize();
    if a.scale() < 2 {
        a.rescale(2);
    }
    a.to_string()
}

/// Why a job of the board gets no bid, from the row alone (spec
/// `board-bidding`, Job choice and Job terms). `None`: the row passes.
pub fn row_filter(
    job: &BoardJob,
    settings: &BoardListing,
    owner: Verification,
    amount: Decimal,
) -> Option<&'static str> {
    if uuid::Uuid::parse_str(&job.job_id).is_err() {
        return Some("bad_job_id");
    }
    if job.job_kind == "competition" {
        return Some("competition");
    }
    if !settings.kinds.contains(&job.job_kind) {
        return Some("kind_not_configured");
    }
    if !matches!(job.status.as_str(), "open" | "filling") || job.current_max_slots < 1 {
        return Some("not_taking_bids");
    }
    if job.is_test_run {
        return Some("test_run");
    }
    if job.requires_verifiable {
        return Some("verifiable_required");
    }
    if job.invited_agent_id.is_some() {
        return Some("quote_request");
    }
    if job.budget_token != "USDC" {
        return Some("usd_job");
    }
    if let Some(b) = &job.budget_amount {
        match Decimal::from_str(b.trim()) {
            Ok(b) if b >= amount => {}
            Ok(_) => return Some("budget_below_amount"),
            Err(_) => return Some("bad_budget"),
        }
    }
    match Verification::parse(&job.min_builder_verification) {
        Some(v) if v <= owner => {}
        _ => return Some("verification_required"),
    }
    if !settings.poster_allowlist.is_empty()
        && !settings
            .poster_allowlist
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&job.creator_account_id))
    {
        return Some("poster_not_allowed");
    }
    None
}

/// The job as a row for the intake functions: the title and description
/// as the brief, the bid amount as the escrow, the job's attachments.
pub fn job_row(job: &BoardJob, amount: Decimal, attachments: Vec<Attachment>) -> Row {
    let v = serde_json::json!({
        "id": format!("board:{}", job.job_id),
        "status": "in_progress",
        "title": job.title,
        "description": job.description,
        "escrowAmount": amount.to_string(),
        "jobId": job.job_id,
    });
    let mut row: Row = serde_json::from_value(v).expect("a board row is a valid row");
    row.job_attachments = attachments;
    row
}

/// The deterministic intake checks on a job (daily loss, hard limits, cost
/// admission, the listing's intake check). `Ok(None)` passes; `Ok(Some)`
/// names the check that declines; `Err` is a check that cannot run now
/// (the daily loss is unknown), which skips the job without a memo.
pub fn deterministic_intake(
    row: &Row,
    ctx: &ListingContext,
    limits: &Limits,
    loss_today: &Result<Decimal, String>,
) -> Result<Option<&'static str>, String> {
    let loss = loss_today.clone()?;
    if matches!(
        intake::daily_loss(loss, limits.daily_loss_usd),
        intake::Outcome::Declined { .. }
    ) {
        // Not memoised: the next UTC day lifts it.
        return Err(intake::DAILY_LOSS.to_owned());
    }
    if matches!(
        intake::hard_limits(row, ctx.limits.as_ref()),
        intake::Outcome::Declined { .. }
    ) {
        return Ok(Some(intake::HARD_LIMITS));
    }
    if matches!(
        intake::cost_admission(crate::cost::ceiling(row, ctx.price_usd), ctx.protection),
        intake::Outcome::Declined { .. }
    ) {
        return Ok(Some(intake::COST));
    }
    if ctx.record.as_ref().and_then(|r| r.intake_check) == Some(listing::IntakeCheck::GithubRepo)
        && matches!(
            intake::github_repo_check(&intake::brief_text(row)),
            intake::Outcome::Declined { .. }
        )
    {
        return Ok(Some(intake::GITHUB_REPO));
    }
    Ok(None)
}

/// Whether the scope question applies: protection on and a scope named.
pub fn scope_due(ctx: &ListingContext) -> bool {
    ctx.protection == Protection::On && ctx.scope.is_some()
}

/// The longest proposal Nearbee sends, in characters.
pub const PROPOSAL_MAX_CHARS: usize = 4000;

/// The bid proposal, from the listing record alone (spec `board-bidding`,
/// Generated proposal): no job text, no model call.
pub fn proposal(ctx: &ListingContext, terms: &Terms) -> String {
    let record = ctx.record.as_ref();
    let scope = ctx
        .scope
        .clone()
        .unwrap_or_else(|| "the service its listing describes".to_owned());
    let tier = record
        .and_then(|r| r.tier)
        .map(|t| t.as_str())
        .unwrap_or("plain");
    let mut models: Vec<String> = Vec::new();
    if let Some(r) = record {
        for m in r.direct_models.iter() {
            let signed = r.signed_models.contains(m);
            models.push(if signed {
                format!("{m} on NEAR AI Cloud (signed response)")
            } else {
                format!("{m} on NEAR AI Cloud")
            });
        }
        for m in &r.script_models {
            match m.split_once('/') {
                Some((agent, model)) => models.push(format!("{model} through {agent}")),
                None => models.push(m.clone()),
            }
        }
    }
    let models = if models.is_empty() {
        "no model call is declared for this listing".to_owned()
    } else {
        models.join(", ")
    };
    let hours = terms.sla_seconds.div_ceil(3600);
    let window = if hours <= 1 {
        "within one hour".to_owned()
    } else {
        format!("within {hours} hours")
    };
    let mut text = format!(
        "I will do this job with the Nearbee agent {handle}: {scope}\n\n\
         How: a fixed playbook runs on the owner's self-hosted machine (privacy tier {tier}); \
         your brief and files are data for that run, never instructions to the agent.\n\
         Models: {models}.\n\
         Delivery: {window} of the award, as Markdown with any files.\n\
         Data handling: the full data statement of this listing is appended to the deliverable.",
        handle = ctx.handle,
    );
    if text.chars().count() > PROPOSAL_MAX_CHARS {
        text = text.chars().take(PROPOSAL_MAX_CHARS).collect();
    }
    text
}

// ---------------------------------------------------------------------------
// The pass
// ---------------------------------------------------------------------------

/// One bid decision, printed by serve at info level. No job text and no
/// proposal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BidEvent {
    Matched {
        handle: String,
        job: String,
    },
    Skipped {
        handle: String,
        job: String,
        reason: String,
    },
    Sent {
        handle: String,
        job: String,
        amount: String,
        bid_id: Option<String>,
    },
    Refused {
        handle: String,
        job: String,
        code: String,
    },
    Status {
        handle: String,
        job: String,
        status: String,
    },
    Awarded {
        handle: String,
        job: String,
        bid_id: String,
    },
    Error {
        handle: String,
        detail: String,
    },
}

impl fmt::Display for BidEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BidEvent::Matched { handle, job } => write!(f, "bid {handle} {job}: matched"),
            BidEvent::Skipped {
                handle,
                job,
                reason,
            } => {
                write!(f, "bid {handle} {job}: skipped ({reason})")
            }
            BidEvent::Sent {
                handle,
                job,
                amount,
                bid_id,
            } => match bid_id {
                Some(b) => write!(f, "bid {handle} {job}: bid sent, USDC {amount}, bid {b}"),
                None => write!(f, "bid {handle} {job}: bid sent, USDC {amount}"),
            },
            BidEvent::Refused { handle, job, code } => {
                write!(f, "bid {handle} {job}: refused ({code})")
            }
            BidEvent::Status {
                handle,
                job,
                status,
            } => write!(f, "bid {handle} {job}: market status {status}"),
            BidEvent::Awarded {
                handle,
                job,
                bid_id,
            } => write!(
                f,
                "bid {handle} {job}: awarded (bid {bid_id}); the hire round serves the assignment"
            ),
            BidEvent::Error { handle, detail } => write!(f, "bid {handle}: error: {detail}"),
        }
    }
}

/// What the bidder keeps between passes, in process memory only: the
/// skip memo (a job's content-based skip, keyed by its `updatedAt`) and
/// the jobs already reported as skipped, so a crowded board is not
/// printed again on every pass.
#[derive(Debug, Default)]
pub struct BidderState {
    memo: BTreeMap<(String, String), (String, String)>,
    /// Jobs whose scope question passed, keyed like the memo with the
    /// job's `updatedAt`: a job held back after the answer (the open-bid
    /// cap, the time budget) is not asked again for the process lifetime.
    scope_passed: BTreeMap<(String, String), String>,
}

/// At most this many scope questions per listing and pass.
pub const SCOPE_QUESTIONS_PER_PASS: usize = 3;
/// The scope question's time limit, as at intake.
pub const SCOPE_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

/// One listing's part of a pass.
pub struct BidListing<'a> {
    pub ctx: &'a ListingContext,
    pub settings: &'a BoardListing,
    pub owner_verification: Verification,
}

/// What the serve loop hands one pass besides the listing.
#[derive(Debug, Clone, Copy)]
pub struct PassInputs<'a> {
    /// The job ids of the listing's assignment rows from this round's hire
    /// read; `None` when that read failed. An awarded bid shows as an
    /// assignment on its job, which is how the bidder learns of the award
    /// without reading a bid list that grows with every hire.
    pub assigned_jobs: Option<&'a BTreeSet<String>>,
    /// When the pass must stop (the time budget of the whole bidder pass
    /// over every listing). `None`: no budget.
    pub deadline: Option<std::time::Instant>,
}

/// The reason a pass stops early when its time budget is spent.
pub const BUDGET_SPENT: &str = "time_budget_spent";

fn remaining(deadline: Option<std::time::Instant>) -> Option<std::time::Duration> {
    deadline.map(|d| d.saturating_duration_since(std::time::Instant::now()))
}

fn spent(deadline: Option<std::time::Instant>) -> bool {
    remaining(deadline).is_some_and(|r| r.is_zero())
}

/// One bidder pass for one listing (design 3). Never fails: every problem
/// is an event.
#[allow(clippy::too_many_arguments)]
pub async fn pass<B: Board, R: Runner>(
    board: &B,
    runner: &R,
    l: &BidListing<'_>,
    state: &mut BidderState,
    state_dir: &Path,
    limits: &Limits,
    clock: &Clock,
    inputs: PassInputs<'_>,
) -> Vec<BidEvent> {
    let handle = l.ctx.handle.clone();
    let mut events = Vec::new();
    let err = |detail: String| BidEvent::Error {
        handle: handle.clone(),
        detail,
    };
    let stop = |events: &mut Vec<BidEvent>| {
        events.push(BidEvent::Error {
            handle: handle.clone(),
            detail: format!("{BUDGET_SPENT}: the rest of this listing waits for the next pass"),
        });
    };
    if spent(inputs.deadline) {
        stop(&mut events);
        return events;
    }
    // 1. The agent's pending bids: without them nothing is bid.
    let bids = match board.pending_bids().await {
        Ok(b) => b,
        Err(e) => {
            events.push(err(format!("cannot read the agent's bids: {e}")));
            return events;
        }
    };
    let by_job: BTreeMap<&str, &MyBid> = bids
        .iter()
        .filter(|b| b.status == "pending")
        .map(|b| (b.job_id.as_str(), b))
        .collect();
    // 2. Settle the journal against the pending bids and the assignments.
    let mut journaled: BTreeSet<String> = BTreeSet::new();
    let mut open_intents = 0u32;
    let mut resend: Vec<BidState> = Vec::new();
    let dir = bids_dir(state_dir, &handle);
    let files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(r) => r
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            events.push(err(format!("cannot list {}: {e}", dir.display())));
            return events;
        }
    };
    for f in files {
        let job = f
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        journaled.insert(job.clone());
        let s = match read_bid(&f) {
            Ok(Some(s)) => s,
            Ok(None) => continue,
            Err(e) => {
                // Counted as bid on: never bid again on that job.
                events.push(err(format!("unreadable bid journal: {e}")));
                continue;
            }
        };
        let pending = by_job.get(job.as_str());
        let assigned = inputs.assigned_jobs.map(|a| a.contains(&job));
        let mut seq = s.records + 1;
        let mut bid_id = s.bid_id.clone();
        if s.is_open_intent() {
            if pending.is_none() && assigned != Some(true) {
                open_intents += 1;
                resend.push(s);
                continue;
            }
            // The bid landed before the crash: pending, or already awarded.
            if let Some(b) = pending {
                bid_id = Some(b.bid_id.clone());
            }
            if let Err(e) = append_bid(
                state_dir,
                &handle,
                &job,
                seq,
                BidEntry::Result {
                    operation_id: s.operation_id.clone(),
                    outcome: "placed".into(),
                    bid_id: bid_id.clone(),
                    code: None,
                },
                clock,
            ) {
                events.push(err(e));
                continue;
            }
            seq += 1;
        } else if s.outcome.as_deref() == Some("refused") {
            continue;
        }
        // Where the bid stands now; `None` when it cannot be told this
        // pass (the assignment read failed and the bid is not pending).
        let now = match (pending, assigned) {
            (Some(b), _) => {
                bid_id = Some(b.bid_id.clone());
                Some("pending")
            }
            (None, Some(true)) => Some("accepted"),
            (None, Some(false)) => Some("closed"),
            (None, None) => None,
        };
        let Some(now) = now else { continue };
        if s.status.as_deref() == Some(now) {
            continue;
        }
        if let Err(e) = append_bid(
            state_dir,
            &handle,
            &job,
            seq,
            BidEntry::Status {
                status: now.to_owned(),
                bid_id: bid_id.clone(),
            },
            clock,
        ) {
            events.push(err(e));
            continue;
        }
        if now == "accepted" {
            events.push(BidEvent::Awarded {
                handle: handle.clone(),
                job: job.clone(),
                bid_id: bid_id.unwrap_or_else(|| "unknown".to_owned()),
            });
        } else {
            events.push(BidEvent::Status {
                handle: handle.clone(),
                job: job.clone(),
                status: now.to_owned(),
            });
        }
    }
    // 3. Resend the intents a crash left open and the market does not
    // show (one send each; `duplicate_bid` closes as placed). Each stays
    // counted as open unless the market refused it.
    for s in resend {
        if spent(inputs.deadline) {
            stop(&mut events);
            return events;
        }
        let req = BidRequest {
            amount: s.amount.clone(),
            proposal: proposal(
                l.ctx,
                &Terms {
                    amount: Decimal::from_str(&s.amount).unwrap_or(l.ctx.price_usd),
                    sla_seconds: s.sla_seconds,
                },
            ),
            sla_seconds: s.sla_seconds,
        };
        if let Some(ev) = send(
            board,
            state_dir,
            &handle,
            &s.job_id,
            &s.operation_id,
            s.records + 1,
            &req,
            clock,
        )
        .await
        {
            if let BidEvent::Refused { .. } = ev {
                open_intents = open_intents.saturating_sub(1);
            }
            events.push(ev);
        }
    }
    // 4. New bids.
    let terms = terms(l.settings, l.ctx);
    let amount = amount_text(terms.amount);
    let mut open = by_job.len() as u32 + open_intents;
    let mut jobs: Vec<BoardJob> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for kind in &l.settings.kinds {
        if spent(inputs.deadline) {
            stop(&mut events);
            return events;
        }
        match board.board_jobs(l.settings.query.as_deref(), kind).await {
            Ok(list) => {
                for j in list {
                    if seen.insert(j.job_id.clone()) {
                        jobs.push(j);
                    }
                }
            }
            Err(e) => events.push(err(format!("cannot read the board ({kind}): {e}"))),
        }
    }
    let mut loss_today: Option<Result<Decimal, String>> = None;
    let mut scope_asked = 0usize;
    for job in jobs {
        let job_id = job.job_id.clone();
        let skip = |reason: &str| BidEvent::Skipped {
            handle: handle.clone(),
            job: job_id.clone(),
            reason: reason.to_owned(),
        };
        if journaled.contains(&job_id) {
            continue;
        }
        if by_job.contains_key(job_id.as_str()) {
            note_skip(
                state,
                &mut events,
                &handle,
                &job,
                "already_bid",
                skip("already_bid"),
            );
            continue;
        }
        let key = (handle.clone(), job_id.clone());
        if let Some((updated, _)) = state.memo.get(&key)
            && *updated == job.updated_at
        {
            continue;
        }
        if let Some(reason) = row_filter(&job, l.settings, l.owner_verification, terms.amount) {
            // The poster's account id (not job text) goes with this skip,
            // so the owner can copy it into the allowlist.
            let shown = if reason == "poster_not_allowed" {
                format!("{reason} {}", job.creator_account_id)
            } else {
                reason.to_owned()
            };
            note_skip(state, &mut events, &handle, &job, reason, skip(&shown));
            continue;
        }
        // The cap comes before the attachment read, the intake and the
        // scope question, so a full listing asks and reads nothing.
        if open >= l.settings.max_open_bids {
            events.push(skip("max_open_bids"));
            continue;
        }
        if spent(inputs.deadline) {
            stop(&mut events);
            return events;
        }
        // The job's files, so the hard limits see what the award will.
        let attachments = match board.job_attachments(&job_id).await {
            Ok(a) => a,
            Err(e) => {
                events.push(skip(&format!(
                    "attachments_unreadable ({})",
                    error_kind(&e)
                )));
                continue;
            }
        };
        let row = job_row(&job, terms.amount, attachments);
        let loss =
            loss_today.get_or_insert_with(|| crate::cost::daily_loss(state_dir, clock.now()));
        match deterministic_intake(&row, l.ctx, limits, loss) {
            Ok(None) => {}
            Ok(Some(check)) => {
                note_skip(state, &mut events, &handle, &job, check, skip(check));
                continue;
            }
            Err(why) => {
                events.push(skip(&why));
                continue;
            }
        }
        let scope_known = state
            .scope_passed
            .get(&key)
            .is_some_and(|u| *u == job.updated_at);
        if scope_due(l.ctx) && !scope_known {
            if scope_asked >= SCOPE_QUESTIONS_PER_PASS {
                events.push(skip("scope_check_next_pass"));
                continue;
            }
            let wait = SCOPE_LIMIT + std::time::Duration::from_secs(5);
            let wait = match remaining(inputs.deadline) {
                Some(r) if r.is_zero() => {
                    stop(&mut events);
                    return events;
                }
                Some(r) => wait.min(r),
                None => wait,
            };
            scope_asked += 1;
            let scope = l.ctx.scope.clone().unwrap_or_default();
            let question = intake::scope_question(&scope, &row);
            let answer = match tokio::time::timeout(
                wait,
                runner.decide(&question, intake::SCOPE_CRITERIA, SCOPE_LIMIT.min(wait)),
            )
            .await
            {
                Ok(Ok(a)) => a,
                _ => crate::runner::DecideOutcome::Unanswered {
                    reason: "error".into(),
                },
            };
            if matches!(
                intake::scope_outcome(&answer, &scope),
                intake::Outcome::Declined { .. }
            ) {
                note_skip(
                    state,
                    &mut events,
                    &handle,
                    &job,
                    intake::SCOPE,
                    skip(intake::SCOPE),
                );
                continue;
            }
            state
                .scope_passed
                .insert(key.clone(), job.updated_at.clone());
        }
        events.push(BidEvent::Matched {
            handle: handle.clone(),
            job: job_id.clone(),
        });
        if spent(inputs.deadline) {
            stop(&mut events);
            return events;
        }
        let req = BidRequest {
            amount: amount.clone(),
            proposal: proposal(l.ctx, &terms),
            sla_seconds: terms.sla_seconds,
        };
        let operation_id = uuid::Uuid::now_v7().to_string();
        if let Err(e) = append_bid(
            state_dir,
            &handle,
            &job_id,
            1,
            BidEntry::Intent {
                operation_id: operation_id.clone(),
                amount: req.amount.clone(),
                sla_seconds: req.sla_seconds,
                proposal_sha256: sha256_hex(req.proposal.as_bytes()),
            },
            clock,
        ) {
            events.push(err(e));
            continue;
        }
        open += 1;
        if let Some(ev) = send(
            board,
            state_dir,
            &handle,
            &job_id,
            &operation_id,
            2,
            &req,
            clock,
        )
        .await
        {
            if let BidEvent::Refused { .. } = ev {
                open = open.saturating_sub(1);
            }
            events.push(ev);
        }
    }
    events
}

/// A short, text-free name of a market error for a skip reason.
fn error_kind(e: &MarketError) -> String {
    match e {
        MarketError::Unauthorized { .. } => "unauthorized".to_owned(),
        MarketError::Refused { status, code, .. } => format!("{status} {code}"),
        MarketError::RetryableRead { .. } => "read failed".to_owned(),
        MarketError::WriteOutcomeUnknown { .. } => "unknown".to_owned(),
        MarketError::Protocol { .. } => "protocol".to_owned(),
    }
}

/// Records a content-based skip in the memo and reports it once per job
/// version.
fn note_skip(
    state: &mut BidderState,
    events: &mut Vec<BidEvent>,
    handle: &str,
    job: &BoardJob,
    reason: &str,
    event: BidEvent,
) {
    state.memo.insert(
        (handle.to_owned(), job.job_id.clone()),
        (job.updated_at.clone(), reason.to_owned()),
    );
    events.push(event);
}

/// A status that says "not now" rather than "no": the bid is sent again.
pub fn is_throttle(status: u16) -> bool {
    matches!(status, 408 | 429)
}

/// Sends one bid whose intent is journaled and records its result. An
/// unknown outcome leaves the intent open for the next pass.
#[allow(clippy::too_many_arguments)]
async fn send<B: Board>(
    board: &B,
    state_dir: &Path,
    handle: &str,
    job_id: &str,
    operation_id: &str,
    seq: u64,
    req: &BidRequest,
    clock: &Clock,
) -> Option<BidEvent> {
    let result = |outcome: &str, bid_id: Option<String>, code: Option<String>| BidEntry::Result {
        operation_id: operation_id.to_owned(),
        outcome: outcome.to_owned(),
        bid_id,
        code,
    };
    let (entry, event) = match board.place_bid(job_id, req).await {
        Ok(PlaceOutcome::Placed { bid_id }) => (
            result("placed", Some(bid_id.clone()), None),
            BidEvent::Sent {
                handle: handle.to_owned(),
                job: job_id.to_owned(),
                amount: req.amount.clone(),
                bid_id: Some(bid_id),
            },
        ),
        Ok(PlaceOutcome::Duplicate) => (
            result("placed", None, Some("duplicate_bid".into())),
            BidEvent::Sent {
                handle: handle.to_owned(),
                job: job_id.to_owned(),
                amount: req.amount.clone(),
                bid_id: None,
            },
        ),
        // Throttled (408, 429): the market did not take the bid now. The
        // intent stays open and the next pass sends it again.
        Err(MarketError::Refused { status, .. }) if is_throttle(status) => {
            return Some(BidEvent::Error {
                handle: handle.to_owned(),
                detail: format!("bid on {job_id} left open: throttled (HTTP {status})"),
            });
        }
        Err(MarketError::Refused { code, .. }) => (
            result("refused", None, Some(code.clone())),
            BidEvent::Refused {
                handle: handle.to_owned(),
                job: job_id.to_owned(),
                code,
            },
        ),
        Err(e) => {
            return Some(BidEvent::Error {
                handle: handle.to_owned(),
                detail: format!("bid on {job_id} left open: {e}"),
            });
        }
    };
    if let Err(e) = append_bid(state_dir, handle, job_id, seq, entry, clock) {
        return Some(BidEvent::Error {
            handle: handle.to_owned(),
            detail: e,
        });
    }
    Some(event)
}
