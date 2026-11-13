//! `nearbee replay` (change `judges-path-and-submission`, design 1, spec
//! `replay-walkthrough`): five hire scenarios through the real
//! `serve::round`, against a scripted market, runner and attestor of this
//! module. It reads no configuration, credential, `.env` or `NEARBEE_`
//! variable, opens no socket and starts no process; its fixtures are
//! embedded in the binary.

mod attestor;
mod market;
mod runner;
mod scenarios;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nearbee_core::config::Limits;
use nearbee_core::journal::{self, Event};
use nearbee_core::listing::{IntakeCheck, ResolvedListing, Tier};
use nearbee_core::market::Row;
use nearbee_core::runner::PlaybookId;
use nearbee_core::serve::{
    self, Agent, Clock, HireOutcome, ListingContext, RoundState, ServeOptions,
};
use time::OffsetDateTime;

use attestor::ReplayAttestor;
use market::ReplayMarket;
use runner::ReplayRunner;

/// The listing the replay serves: `repo-audit-fast` as published after
/// this change.
pub const HANDLE: &str = "repo-audit-fast";
/// Its one direct and signed model.
pub const MODEL: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";
pub const PLAYBOOK_VERSION: &str = "1.0.0";
const SCOPE: &str =
    "An audit of one public GitHub repository named by its https://github.com URL in the brief.";
const RETENTION_DAYS: u32 = 7;
/// Seconds the replay clock moves between rounds (serve's default poll).
const POLL_SECS: i64 = 45;
/// The most rounds one step of a scenario may take.
const MAX_ROUNDS: usize = 12;
/// The replay clock's first instant.
const START: OffsetDateTime = time::macros::datetime!(2026-11-12 12:00:00 UTC);

/// The scenarios, in the order they run.
pub const SCENARIOS: [&str; 5] = ["success", "decline", "redo", "restart", "cancellation"];

/// The banner every run starts with (spec `replay-walkthrough`, Honest
/// narration).
pub const BANNER: [&str; 4] = [
    "Nearbee replay: a scripted walkthrough of the hire state machine that contacts nothing (no market, no apb, no NEAR AI Cloud).",
    "Market rows are shaped like the recorded market answers; the transitions are scripted. Serve's real round, journal, intake, tier check, manifest validation and signature recheck run unchanged.",
    "The response signature and its receipt link are synthetic placeholders and will not verify in the NEAR AI Verification Center.",
    "Live runs are recorded in docs/demo-log.md.",
];

/// The replay's options.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The scenarios to run; empty runs all of them.
    pub scenarios: Vec<String>,
    /// Keep the state in this directory (new or empty) instead of a
    /// removed temporary one.
    pub keep: Option<PathBuf>,
    /// Name the journal events each step wrote.
    pub verbose: bool,
}

/// Runs the replay; returns the exit code: 0 all passed, 1 one failed, 2
/// a usage error.
pub async fn run(opts: &Options, out: &mut dyn Write) -> i32 {
    let selected: Vec<&str> = if opts.scenarios.is_empty() {
        SCENARIOS.to_vec()
    } else {
        let mut picked = Vec::new();
        for name in &opts.scenarios {
            match SCENARIOS.iter().find(|s| **s == name.as_str()) {
                Some(s) => picked.push(*s),
                None => {
                    let _ = writeln!(
                        out,
                        "error: unknown scenario `{name}`; the scenarios are {}",
                        SCENARIOS.join(", ")
                    );
                    return 2;
                }
            }
        }
        picked
    };
    // The state directory: a kept one must be new or empty.
    let temp;
    let root: PathBuf = match &opts.keep {
        Some(dir) => {
            match std::fs::read_dir(dir) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        let _ = writeln!(
                            out,
                            "error: --keep {} is not empty; name a new or empty directory",
                            dir.display()
                        );
                        return 2;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        let _ = writeln!(out, "error: --keep {}: {e}", dir.display());
                        return 2;
                    }
                }
                Err(e) => {
                    let _ = writeln!(out, "error: --keep {}: {e}", dir.display());
                    return 2;
                }
            }
            dir.clone()
        }
        None => match tempfile::Builder::new().prefix("nearbee-replay-").tempdir() {
            Ok(t) => {
                temp = t;
                temp.path().to_path_buf()
            }
            Err(e) => {
                let _ = writeln!(out, "error: no temporary directory: {e}");
                return 1;
            }
        },
    };
    for line in BANNER {
        let _ = writeln!(out, "{line}");
    }
    let mut results = Vec::new();
    for name in &selected {
        let _ = writeln!(out);
        let mut world = match World::new(&root.join(name), opts.verbose) {
            Ok(w) => w,
            Err(e) => {
                let _ = writeln!(out, "scenario {name}: failed: {e}");
                results.push((*name, false));
                continue;
            }
        };
        let verdict = scenarios::run(name, &mut world, out).await;
        let ok = verdict.is_ok();
        match verdict {
            Ok(()) => {
                let _ = writeln!(out, "scenario {name}: ok");
            }
            Err(e) => {
                let _ = writeln!(out, "scenario {name}: failed: {e}");
            }
        }
        results.push((*name, ok));
    }
    let _ = writeln!(out);
    for (name, ok) in &results {
        let _ = writeln!(out, "{name:<13} {}", if *ok { "ok" } else { "failed" });
    }
    let passed = results.iter().filter(|(_, ok)| *ok).count();
    let _ = writeln!(out, "replay: {passed} of {} scenarios ok", results.len());
    if let Some(dir) = &opts.keep {
        let _ = writeln!(out, "the state is kept in {}", dir.display());
    }
    if passed == results.len() { 0 } else { 1 }
}

/// One scenario's world: the scripted market, runner and attestor, the
/// serve state, the clock and the listing.
pub(crate) struct World {
    state_dir: PathBuf,
    market: ReplayMarket,
    runner: ReplayRunner,
    attestor: ReplayAttestor,
    state: RoundState,
    now: Arc<Mutex<OffsetDateTime>>,
    clock: Clock,
    ctx: ListingContext,
    opts: ServeOptions,
    limits: Limits,
    rounds: usize,
    verbose: bool,
}

/// The listing record of `repo-audit-fast` with the statement, the
/// fingerprint and the evidence set rendered from the replay's facts.
fn record() -> ResolvedListing {
    let direct = vec![MODEL.to_owned()];
    let facts = runner::facts().with_direct_models(&direct, &direct);
    let statement = nearbee_core::statement::render(Tier::Attested, &facts, RETENTION_DAYS);
    ResolvedListing {
        handle: HANDLE.into(),
        agent_id: "replay-agent".into(),
        playbook_id: HANDLE.into(),
        playbook_version: PLAYBOOK_VERSION.into(),
        playbook_sha256: journal::sha256_hex(b"replay playbook"),
        price_usd: "0.50".into(),
        sla_seconds: 86_400,
        scope: Some(SCOPE.into()),
        protection: None,
        limits: None,
        tier: Some(Tier::Attested),
        intake_check: Some(IntakeCheck::GithubRepo),
        statement_sha256: Some(journal::sha256_hex(statement.as_bytes())),
        statement: Some(statement),
        statement_format: Some(nearbee_core::statement::STATEMENT_FORMAT),
        fingerprint: Some(nearbee_core::statement::fingerprint(Tier::Attested, &facts)),
        evidence_models: nearbee_core::tiers::evidence_models(&facts),
        direct_models: direct.clone(),
        signed_models: direct,
        script_models: Vec::new(),
        script_accounts: Default::default(),
        published_at: START - time::Duration::days(1),
    }
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

impl World {
    fn new(dir: &Path, verbose: bool) -> Result<Self, String> {
        let state_dir = dir.join("state");
        let workspace = dir.join("workspace");
        for d in [&state_dir, &workspace] {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let now = Arc::new(Mutex::new(START));
        let cell = now.clone();
        let clock = Clock::new(move || *cell.lock().unwrap_or_else(|p| p.into_inner()));
        let record = record();
        let ctx = ListingContext {
            handle: HANDLE.into(),
            agent_id: record.agent_id.clone(),
            playbook: PlaybookId::new(HANDLE),
            workspace,
            price_usd: rust_decimal::Decimal::new(50, 2),
            scope: Some(SCOPE.into()),
            protection: nearbee_core::listing::Protection::On,
            limits: None,
            sla_seconds: Some(record.sla_seconds),
            playbook_version: PLAYBOOK_VERSION.into(),
            playbook_sha256: record.playbook_sha256.clone(),
            record: Some(record),
        };
        Ok(Self {
            state_dir,
            market: ReplayMarket::new(now.clone()),
            runner: ReplayRunner::new(),
            attestor: ReplayAttestor::default(),
            state: RoundState::default(),
            now,
            clock,
            ctx,
            opts: ServeOptions {
                poll_interval_secs: POLL_SECS as u64,
                run_poll_interval: std::time::Duration::ZERO,
                progress_every: None,
                reject_handle: None,
                sla_margin_minutes: 10,
                forbidden: Vec::new(),
                statement_retention_days: RETENTION_DAYS,
            },
            limits: Limits::default(),
            rounds: 0,
            verbose,
        })
    }

    fn now(&self) -> OffsetDateTime {
        *self.now.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A new first-time row with `brief`, shaped like a recorded market
    /// assignment (camelCase fields, the same optional ones).
    fn hire_arrives(&self, id: &str, brief: &str) -> Result<(), String> {
        let now = self.now();
        let row: Row = serde_json::from_value(serde_json::json!({
            "id": id,
            "status": "in_progress",
            "createdAt": rfc3339(now - time::Duration::minutes(1)),
            "updatedAt": rfc3339(now - time::Duration::minutes(1)),
            "slaSeconds": 86_400,
            "slaDeadlineAt": rfc3339(now + time::Duration::days(1)),
            "escrowAmount": "0.50",
            "escrowToken": "USD",
            "jobId": format!("{id}-job"),
            "title": "Audit a webhook library",
            "description": brief,
            "jobAttachments": [],
            "latestMessage": null
        }))
        .map_err(|e| format!("the scripted row does not parse: {e}"))?;
        self.market.edit(|rows| rows.push(row));
        Ok(())
    }

    /// The hire's journal, read fresh.
    fn journal(&self, id: &str) -> Option<journal::JournalRead> {
        let text = std::fs::read_to_string(journal::journal_path(&self.state_dir, id)).ok()?;
        journal::read(&text).ok()
    }

    /// The hire's newest lifecycle status.
    fn lifecycle(&self, id: &str) -> Option<String> {
        self.journal(id)?
            .records
            .iter()
            .rev()
            .find_map(|r| match &r.event {
                Event::Lifecycle(l) => Some(l.status.clone()),
                _ => None,
            })
    }

    fn records(&self, id: &str) -> usize {
        self.journal(id).map_or(0, |j| j.records.len())
    }

    /// One serve round, then the clock moves on; prints the step line.
    async fn round(&mut self, id: &str, out: &mut dyn Write) -> Result<(), String> {
        self.rounds += 1;
        let before = self.records(id);
        let agents = vec![Agent {
            ctx: self.ctx.clone(),
            rows: self.market.rows(),
            market: &self.market,
        }];
        let outcomes = serve::round(
            &self.runner,
            &self.attestor,
            &agents,
            &mut self.state,
            &self.opts,
            &self.state_dir,
            &self.limits,
            &self.clock,
        )
        .await
        .map_err(|e| format!("round {}: {e}", self.rounds))?;
        drop(agents);
        let state = outcomes
            .iter()
            .filter(|o| o.row_id() == id)
            .map(|o| self.describe(o))
            .collect::<Vec<_>>();
        let state = if state.is_empty() {
            "nothing to do".to_owned()
        } else {
            state.join(", ")
        };
        let mut events = self.market.take_events();
        events.extend(self.runner.take_events());
        events.extend(self.attestor.take_events());
        let mut line = format!("  {:>2}. hire: {state}", self.rounds);
        for e in events {
            line.push_str("; ");
            line.push_str(&e);
        }
        if self.verbose
            && let Some(j) = self.journal(id)
        {
            let names: Vec<String> = j.records[before.min(j.records.len())..]
                .iter()
                .map(|r| match &r.event {
                    Event::Intent(i) => format!("intent {}", i.kind),
                    e => e.event_type().to_owned(),
                })
                .collect();
            if !names.is_empty() {
                line.push_str(&format!(" [journal: {}]", names.join(", ")));
            }
        }
        let _ = writeln!(out, "{line}");
        *self.now.lock().unwrap_or_else(|p| p.into_inner()) += time::Duration::seconds(POLL_SECS);
        Ok(())
    }

    /// A hire outcome in words, without any identifier or path.
    fn describe(&self, o: &HireOutcome) -> String {
        match o {
            HireOutcome::Delivered { .. } => "delivered".into(),
            HireOutcome::DeclinedRejected { .. } => "declined rejected".into(),
            HireOutcome::DeclinedFailed { reason, .. } => format!("declined failed ({reason})"),
            HireOutcome::Left { why, .. } => (*why).to_owned(),
            HireOutcome::OperatorNeeded { why, .. } => {
                let root = self.state_dir.parent().unwrap_or(&self.state_dir);
                format!(
                    "operator told: {}",
                    why.replace(&root.to_string_lossy().into_owned(), "<state>")
                )
            }
            HireOutcome::LookupIncomplete { .. } => "run lookup incomplete".into(),
            HireOutcome::Working { .. } => "working".into(),
        }
    }

    /// Rounds until `done` holds (at most [`MAX_ROUNDS`]), under a
    /// heading.
    async fn step(
        &mut self,
        id: &str,
        title: &str,
        out: &mut dyn Write,
        done: impl Fn(&World) -> bool,
    ) -> Result<(), String> {
        let _ = writeln!(out, "  {title}");
        for _ in 0..MAX_ROUNDS {
            self.round(id, out).await?;
            if done(self) {
                return Ok(());
            }
        }
        Err(format!("{title}: not reached in {MAX_ROUNDS} rounds"))
    }
}
