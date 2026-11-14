//! The bidder (change `board-bids`, spec `board-bidding`): the bid
//! journal, the choice of jobs, the generated proposal and whole passes
//! against a scripted board and a scripted scope answer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nearbee_core::board::{
    self, BidEntry, BidEvent, BidListing, BidRequest, BidderState, Board, BoardJob, MyBid,
    PassInputs, PlaceOutcome,
};
use nearbee_core::config::{BoardListing, Limits, Verification};
use nearbee_core::market::{Attachment, MarketError};
use nearbee_core::runner::{
    DecideOutcome, Estimate, ExecutionFacts, PlaybookId, PlaybookSummary, RunDetail, RunId,
    RunLookup, RunQuery, Runner, RunnerError, StartRequest, WorkspaceId,
};
use nearbee_core::serve::{Clock, ListingContext};
use rust_decimal::Decimal;
use time::macros::datetime;

const POSTER: &str = "1729637e-8362-4026-98c3-5d597983f018";

fn job_id(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn job(n: u32) -> BoardJob {
    BoardJob {
        job_id: job_id(n),
        creator_account_id: POSTER.into(),
        title: "Build a small website".into(),
        description: "A landing page with Astro for a bakery.".into(),
        budget_amount: Some("5".into()),
        budget_token: "USDC".into(),
        job_kind: "standard".into(),
        status: "open".into(),
        current_max_slots: 1,
        min_builder_verification: "none".into(),
        is_test_run: false,
        invited_agent_id: None,
        requires_verifiable: false,
        updated_at: "2026-11-14T10:00:00Z".into(),
    }
}

fn settings() -> BoardListing {
    BoardListing {
        query: Some("website".into()),
        kinds: vec!["standard".into()],
        amount_usd: Some(Decimal::new(200, 2)),
        sla_seconds: Some(86_400),
        max_open_bids: 3,
        poster_allowlist: Vec::new(),
    }
}

fn record() -> nearbee_core::listing::ResolvedListing {
    nearbee_core::listing::ResolvedListing {
        handle: "astro-site".into(),
        agent_id: "ag_1".into(),
        playbook_id: "astro-site".into(),
        playbook_version: "1.0.1".into(),
        playbook_sha256: "ab".into(),
        price_usd: "1.00".into(),
        sla_seconds: 86_400,
        scope: Some("Builds a static website with Astro from a brief.".into()),
        protection: None,
        limits: None,
        tier: Some(nearbee_core::listing::Tier::Plain),
        intake_check: None,
        statement: Some("## Data handling".into()),
        statement_sha256: None,
        statement_format: None,
        fingerprint: None,
        evidence_models: Vec::new(),
        direct_models: Vec::new(),
        signed_models: Vec::new(),
        script_models: vec!["claude/claude-sonnet-5-5".into()],
        script_accounts: Default::default(),
        published_at: datetime!(2026-11-14 09:00 UTC),
    }
}

fn ctx(scope: bool) -> ListingContext {
    ListingContext {
        handle: "astro-site".into(),
        agent_id: "ag_1".into(),
        playbook: PlaybookId::new("astro-site"),
        workspace: PathBuf::from("/ws"),
        price_usd: Decimal::new(100, 2),
        scope: scope.then(|| "Builds a static website with Astro from a brief.".to_owned()),
        protection: nearbee_core::listing::Protection::On,
        limits: None,
        sla_seconds: Some(86_400),
        playbook_version: "1.0.1".into(),
        playbook_sha256: "ab".into(),
        record: Some(record()),
    }
}

fn clock() -> Clock {
    Clock::new(|| datetime!(2026-11-14 12:00 UTC))
}

/// A scripted board: the jobs it lists, the bids it holds, and what a
/// placement answers. Every placement is recorded.
#[derive(Default)]
struct FakeBoard {
    jobs: Vec<BoardJob>,
    /// Every bid in every status; the market lists only the pending ones.
    bids: Mutex<Vec<MyBid>>,
    /// Answers for placements, in order; empty answers `Placed`.
    answers: Mutex<Vec<Result<PlaceOutcome, MarketError>>>,
    placed: Mutex<Vec<(String, BidRequest)>>,
    bids_fail: bool,
    /// The job ids with an assignment (`None`: the hire read failed).
    assigned: Mutex<Option<BTreeSet<String>>>,
    /// Attachments per job; a job named in `attachments_fail` cannot be read.
    attachments: BTreeMap<String, Vec<Attachment>>,
    attachments_fail: BTreeSet<String>,
    attachment_reads: Mutex<u32>,
    /// How often the pending bids were read.
    pending_reads: Mutex<u32>,
}

impl Board for FakeBoard {
    async fn board_jobs(
        &self,
        _query: Option<&str>,
        kind: &str,
    ) -> Result<Vec<BoardJob>, MarketError> {
        Ok(self
            .jobs
            .iter()
            .filter(|j| j.job_kind == kind || kind == "standard")
            .cloned()
            .collect())
    }
    async fn pending_bids(&self) -> Result<Vec<MyBid>, MarketError> {
        *self.pending_reads.lock().unwrap() += 1;
        if self.bids_fail {
            return Err(MarketError::RetryableRead {
                detail: "down".into(),
                retry_after_secs: None,
            });
        }
        Ok(self
            .bids
            .lock()
            .unwrap()
            .iter()
            .filter(|b| b.status == "pending")
            .cloned()
            .collect())
    }
    async fn job_attachments(&self, job_id: &str) -> Result<Vec<Attachment>, MarketError> {
        *self.attachment_reads.lock().unwrap() += 1;
        if self.attachments_fail.contains(job_id) {
            return Err(MarketError::Refused {
                status: 403,
                code: "forbidden".into(),
                detail: "not a party".into(),
            });
        }
        Ok(self.attachments.get(job_id).cloned().unwrap_or_default())
    }
    async fn place_bid(&self, job_id: &str, req: &BidRequest) -> Result<PlaceOutcome, MarketError> {
        self.placed
            .lock()
            .unwrap()
            .push((job_id.to_owned(), req.clone()));
        let mut a = self.answers.lock().unwrap();
        if a.is_empty() {
            let bid_id = format!("bid-{job_id}");
            self.bids.lock().unwrap().push(MyBid {
                bid_id: bid_id.clone(),
                job_id: job_id.to_owned(),
                status: "pending".into(),
                amount: req.amount.clone(),
            });
            return Ok(PlaceOutcome::Placed { bid_id });
        }
        a.remove(0)
    }
}

/// A runner that only answers the scope question.
struct ScopeRunner {
    yes: bool,
    asked: Mutex<u32>,
    /// How long each answer takes.
    delay: std::time::Duration,
}

impl ScopeRunner {
    fn new(yes: bool) -> Self {
        Self {
            yes,
            asked: Mutex::new(0),
            delay: std::time::Duration::ZERO,
        }
    }
}

impl Runner for ScopeRunner {
    fn workspace(&self) -> &WorkspaceId {
        unimplemented!()
    }
    async fn playbooks(&self) -> Result<Vec<PlaybookSummary>, RunnerError> {
        unimplemented!()
    }
    async fn estimate(&self, _id: &PlaybookId) -> Result<Estimate, RunnerError> {
        unimplemented!()
    }
    async fn start(&self, _req: StartRequest) -> Result<RunId, RunnerError> {
        unimplemented!()
    }
    async fn external_data_supported(&self) -> Result<bool, RunnerError> {
        unimplemented!()
    }
    async fn run(&self, _id: &RunId) -> Result<RunDetail, RunnerError> {
        unimplemented!()
    }
    async fn stop(&self, _id: &RunId) -> Result<(), RunnerError> {
        unimplemented!()
    }
    async fn abort(&self, _id: &RunId) -> Result<(), RunnerError> {
        unimplemented!()
    }
    async fn decide(
        &self,
        _question: &str,
        _criteria: &str,
        _limit: std::time::Duration,
    ) -> Result<DecideOutcome, RunnerError> {
        *self.asked.lock().unwrap() += 1;
        tokio::time::sleep(self.delay).await;
        Ok(DecideOutcome::Answered {
            yes: self.yes,
            p: Some(if self.yes {
                Decimal::new(9, 1)
            } else {
                Decimal::new(5, 2)
            }),
        })
    }
    async fn find(&self, _q: RunQuery) -> Result<RunLookup, RunnerError> {
        unimplemented!()
    }
    async fn inspect(&self, _id: &PlaybookId) -> Result<ExecutionFacts, RunnerError> {
        unimplemented!()
    }
    async fn inspect_source(&self, _source: &str) -> Result<ExecutionFacts, RunnerError> {
        unimplemented!()
    }
}

async fn run_pass(
    board: &FakeBoard,
    runner: &ScopeRunner,
    c: &ListingContext,
    s: &BoardListing,
    state: &mut BidderState,
    dir: &Path,
) -> Vec<BidEvent> {
    let l = BidListing {
        ctx: c,
        settings: s,
        owner_verification: Verification::None,
    };
    let assigned = board.assigned.lock().unwrap().clone();
    board::pass(
        board,
        runner,
        &l,
        state,
        dir,
        &Limits::default(),
        &clock(),
        PassInputs {
            assigned_jobs: assigned.as_ref(),
            deadline: None,
        },
    )
    .await
}

fn skipped(events: &[BidEvent], job: &str) -> Option<String> {
    events.iter().find_map(|e| match e {
        BidEvent::Skipped { job: j, reason, .. } if j == job => Some(reason.clone()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Job choice
// ---------------------------------------------------------------------------

/// A change to a matching job that makes the filter skip it.
type JobChange = Box<dyn Fn(&mut BoardJob)>;

#[test]
fn every_row_filter_names_its_reason() {
    let s = settings();
    let amount = Decimal::new(200, 2);
    let check = |j: BoardJob, s: &BoardListing, owner: Verification| {
        board::row_filter(&j, s, owner, amount)
    };
    assert_eq!(check(job(1), &s, Verification::None), None);
    let mut no_budget = job(1);
    no_budget.budget_amount = None;
    assert_eq!(check(no_budget, &s, Verification::None), None);
    let cases: Vec<(JobChange, &str)> = vec![
        (
            Box::new(|j| j.budget_amount = Some("1.00".into())),
            "budget_below_amount",
        ),
        (Box::new(|j| j.budget_token = "USD".into()), "usd_job"),
        (
            Box::new(|j| j.job_kind = "instant".into()),
            "kind_not_configured",
        ),
        (
            Box::new(|j| j.job_kind = "competition".into()),
            "competition",
        ),
        (
            Box::new(|j| j.status = "in_progress".into()),
            "not_taking_bids",
        ),
        (Box::new(|j| j.current_max_slots = 0), "not_taking_bids"),
        (Box::new(|j| j.is_test_run = true), "test_run"),
        (
            Box::new(|j| j.requires_verifiable = true),
            "verifiable_required",
        ),
        (
            Box::new(|j| j.invited_agent_id = Some("x".into())),
            "quote_request",
        ),
        (
            Box::new(|j| j.min_builder_verification = "verified".into()),
            "verification_required",
        ),
        (
            Box::new(|j| j.min_builder_verification = "platinum".into()),
            "verification_required",
        ),
        (Box::new(|j| j.job_id = "../../etc".into()), "bad_job_id"),
    ];
    for (change, reason) in cases {
        let mut j = job(1);
        change(&mut j);
        assert_eq!(check(j, &s, Verification::None), Some(reason), "{reason}");
    }
    // A verified owner meets `verified`.
    let mut v = job(1);
    v.min_builder_verification = "verified".into();
    assert_eq!(check(v, &s, Verification::Verified), None);
    // The allowlist, case-insensitive.
    let mut allow = settings();
    allow.poster_allowlist = vec!["22222222-2222-4222-8222-222222222222".into()];
    assert_eq!(
        check(job(1), &allow, Verification::None),
        Some("poster_not_allowed")
    );
    allow.poster_allowlist = vec![POSTER.to_uppercase()];
    assert_eq!(check(job(1), &allow, Verification::None), None);
}

#[test]
fn terms_default_to_the_listing_price_and_sla() {
    let c = ctx(false);
    let mut s = settings();
    s.amount_usd = None;
    s.sla_seconds = None;
    let t = board::terms(&s, &c);
    assert_eq!(t.amount, Decimal::new(100, 2));
    assert_eq!(t.sla_seconds, 86_400);
    assert_eq!(board::amount_text(t.amount), "1.00");
    assert_eq!(board::amount_text(Decimal::new(2, 0)), "2.00");
    assert_eq!(board::amount_text(Decimal::new(2505, 3)), "2.505");
}

// ---------------------------------------------------------------------------
// The proposal
// ---------------------------------------------------------------------------

#[test]
fn the_proposal_comes_from_the_listing_and_never_from_the_job() {
    let c = ctx(true);
    let t = board::terms(&settings(), &c);
    let p = board::proposal(&c, &t);
    assert!(p.contains("astro-site"), "{p}");
    assert!(p.contains("Builds a static website with Astro"), "{p}");
    assert!(p.contains("claude-sonnet-5-5 through claude"), "{p}");
    assert!(p.contains("privacy tier plain"), "{p}");
    assert!(p.contains("within 24 hours"), "{p}");
    assert!(p.contains("data statement"), "{p}");
    assert!(!p.contains("bakery"), "{p}");
    assert!(!p.contains('\u{2014}') && !p.contains('\u{2013}') && !p.contains('!'));
    assert!(p.chars().count() <= board::PROPOSAL_MAX_CHARS);
}

// ---------------------------------------------------------------------------
// The bid journal
// ---------------------------------------------------------------------------

#[test]
fn the_bid_journal_reads_back_its_states() {
    let dir = tempfile::tempdir().unwrap();
    let j = job_id(7);
    let intent = BidEntry::Intent {
        operation_id: "op".into(),
        amount: "2.00".into(),
        sla_seconds: 3600,
        proposal_sha256: "aa".into(),
    };
    board::append_bid(dir.path(), "astro-site", &j, 1, intent, &clock()).unwrap();
    let path = board::bid_path(dir.path(), "astro-site", &j);
    let s = board::read_bid(&path).unwrap().unwrap();
    assert_eq!(s.state(), "placing");
    assert!(s.is_open_intent());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    board::append_bid(
        dir.path(),
        "astro-site",
        &j,
        2,
        BidEntry::Result {
            operation_id: "op".into(),
            outcome: "placed".into(),
            bid_id: Some("b1".into()),
            code: None,
        },
        &clock(),
    )
    .unwrap();
    assert_eq!(board::read_bid(&path).unwrap().unwrap().state(), "placed");
    board::append_bid(
        dir.path(),
        "astro-site",
        &j,
        3,
        BidEntry::Status {
            status: "accepted".into(),
            bid_id: Some("b1".into()),
        },
        &clock(),
    )
    .unwrap();
    let s = board::read_bid(&path).unwrap().unwrap();
    assert_eq!(s.state(), "accepted");
    assert_eq!(s.bid_id.as_deref(), Some("b1"));
    // A torn line is an error, never an empty journal.
    std::fs::write(&path, "{\"seq\":1,").unwrap();
    assert!(board::read_bid(&path).is_err());
    let all = board::list_bids(dir.path()).unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].is_err());
}

// ---------------------------------------------------------------------------
// Passes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_matching_job_gets_one_bid_and_a_second_pass_bids_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(1)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let c = ctx(true);
    let s = settings();
    let mut state = BidderState::default();
    let ev = run_pass(&board, &runner, &c, &s, &mut state, dir.path()).await;
    assert!(
        ev.iter().any(|e| matches!(e, BidEvent::Sent { job, amount, .. } if *job == job_id(1) && amount == "2.00")),
        "{ev:?}"
    );
    let placed = board.placed.lock().unwrap().clone();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].1.sla_seconds, 86_400);
    assert!(!placed[0].1.proposal.contains("bakery"));
    // The journal: intent, result placed, the digest of the proposal only.
    let path = board::bid_path(dir.path(), "astro-site", &job_id(1));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("bakery") && !text.contains("Astro"),
        "{text}"
    );
    assert_eq!(board::read_bid(&path).unwrap().unwrap().state(), "placed");
    // A restart (a fresh state) bids nothing on the same job.
    let mut fresh = BidderState::default();
    run_pass(&board, &runner, &c, &s, &mut fresh, dir.path()).await;
    assert_eq!(board.placed.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_intent_left_open_is_closed_from_the_market_without_a_second_bid() {
    let dir = tempfile::tempdir().unwrap();
    let j = job_id(2);
    board::append_bid(
        dir.path(),
        "astro-site",
        &j,
        1,
        BidEntry::Intent {
            operation_id: "op".into(),
            amount: "2.00".into(),
            sla_seconds: 86_400,
            proposal_sha256: "aa".into(),
        },
        &clock(),
    )
    .unwrap();
    // The bid landed before the crash: the market shows it.
    let board = FakeBoard {
        jobs: vec![job(2)],
        bids: Mutex::new(vec![MyBid {
            bid_id: "b2".into(),
            job_id: j.clone(),
            status: "pending".into(),
            amount: "2.00".into(),
        }]),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(board.placed.lock().unwrap().is_empty());
    let s = board::read_bid(&board::bid_path(dir.path(), "astro-site", &j))
        .unwrap()
        .unwrap();
    assert_eq!(s.outcome.as_deref(), Some("placed"));
    assert_eq!(s.bid_id.as_deref(), Some("b2"));
    assert_eq!(s.state(), "pending");
}

#[tokio::test]
async fn an_intent_the_market_does_not_show_is_resent_and_a_duplicate_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let j = job_id(3);
    board::append_bid(
        dir.path(),
        "astro-site",
        &j,
        1,
        BidEntry::Intent {
            operation_id: "op".into(),
            amount: "2.00".into(),
            sla_seconds: 86_400,
            proposal_sha256: "aa".into(),
        },
        &clock(),
    )
    .unwrap();
    let board = FakeBoard {
        answers: Mutex::new(vec![Ok(PlaceOutcome::Duplicate)]),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(board.placed.lock().unwrap().len(), 1);
    let s = board::read_bid(&board::bid_path(dir.path(), "astro-site", &j))
        .unwrap()
        .unwrap();
    assert_eq!(s.outcome.as_deref(), Some("placed"));
    assert_eq!(s.code.as_deref(), Some("duplicate_bid"));
}

#[tokio::test]
async fn a_refused_bid_is_recorded_and_never_retried() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(4)],
        answers: Mutex::new(vec![Err(MarketError::Refused {
            status: 409,
            code: "cluster_already_in_job".into(),
            detail: "another agent".into(),
        })]),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(
        ev.iter().any(
            |e| matches!(e, BidEvent::Refused { code, .. } if code == "cluster_already_in_job")
        ),
        "{ev:?}"
    );
    let s = board::read_bid(&board::bid_path(dir.path(), "astro-site", &job_id(4)))
        .unwrap()
        .unwrap();
    assert_eq!(s.state(), "refused (cluster_already_in_job)");
    let mut fresh = BidderState::default();
    run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut fresh,
        dir.path(),
    )
    .await;
    assert_eq!(board.placed.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_bid_already_on_the_market_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(5)],
        bids: Mutex::new(vec![MyBid {
            bid_id: "manual".into(),
            job_id: job_id(5),
            status: "pending".into(),
            amount: "3.00".into(),
        }]),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(skipped(&ev, &job_id(5)).as_deref(), Some("already_bid"));
    assert!(board.placed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_open_bid_cap_holds_back_new_bids() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(10), job(11), job(12)],
        bids: Mutex::new(
            (0..2)
                .map(|i| MyBid {
                    bid_id: format!("old-{i}"),
                    job_id: format!("other-{i}"),
                    status: "pending".into(),
                    amount: "2.00".into(),
                })
                .collect(),
        ),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    // 2 pending + 1 new = the cap of 3.
    assert_eq!(board.placed.lock().unwrap().len(), 1);
    assert_eq!(skipped(&ev, &job_id(11)).as_deref(), Some("max_open_bids"));
    assert_eq!(skipped(&ev, &job_id(12)).as_deref(), Some("max_open_bids"));
    // Not memoised: once a bid is no longer pending, the next pass bids.
    for b in board.bids.lock().unwrap().iter_mut() {
        if b.job_id.starts_with("other-") {
            b.status = "rejected".into();
        }
    }
    run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(board.placed.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn an_out_of_scope_job_gets_no_bid_and_is_asked_once() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(6)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(false);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(skipped(&ev, &job_id(6)).as_deref(), Some("scope"));
    let ev = run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(skipped(&ev, &job_id(6)).is_none(), "{ev:?}");
    assert_eq!(*runner.asked.lock().unwrap(), 1);
    assert!(board.placed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_brief_over_the_hard_limit_gets_no_bid() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = ctx(false);
    c.limits = Some(nearbee_core::listing::ListingLimits {
        max_brief_bytes: Some(10),
        ..Default::default()
    });
    let board = FakeBoard {
        jobs: vec![job(8)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(&board, &runner, &c, &settings(), &mut state, dir.path()).await;
    assert_eq!(skipped(&ev, &job_id(8)).as_deref(), Some("hard_limits"));
    assert!(board.placed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_award_is_recorded_once_and_logged() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(9)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    board.bids.lock().unwrap()[0].status = "accepted".into();
    *board.assigned.lock().unwrap() = Some([job_id(9)].into_iter().collect());
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    let awarded: Vec<String> = ev
        .iter()
        .filter(|e| matches!(e, BidEvent::Awarded { .. }))
        .map(ToString::to_string)
        .collect();
    assert_eq!(awarded.len(), 1, "{ev:?}");
    assert!(awarded[0].contains(&job_id(9)) && awarded[0].contains("astro-site"));
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(!ev.iter().any(|e| matches!(e, BidEvent::Awarded { .. })));
    let s = board::read_bid(&board::bid_path(dir.path(), "astro-site", &job_id(9)))
        .unwrap()
        .unwrap();
    assert_eq!(s.state(), "accepted");
}

#[tokio::test]
async fn no_bid_without_the_agents_bids() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(1)],
        bids_fail: true,
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(matches!(ev.as_slice(), [BidEvent::Error { .. }]), "{ev:?}");
    assert!(board.placed.lock().unwrap().is_empty());
}

#[test]
fn event_lines_carry_no_job_text() {
    let e = BidEvent::Skipped {
        handle: "astro-site".into(),
        job: job_id(1),
        reason: "kind_not_configured".into(),
    };
    assert_eq!(
        e.to_string(),
        format!(
            "bid astro-site {}: skipped (kind_not_configured)",
            job_id(1)
        )
    );
}

#[tokio::test]
async fn a_poster_outside_the_allowlist_is_named_by_account_id() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(13)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut s = settings();
    s.poster_allowlist = vec!["22222222-2222-4222-8222-222222222222".into()];
    let mut state = BidderState::default();
    let ev = run_pass(&board, &runner, &ctx(false), &s, &mut state, dir.path()).await;
    assert_eq!(
        skipped(&ev, &job_id(13)),
        Some(format!("poster_not_allowed {POSTER}"))
    );
    assert!(board.placed.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Review fixes (one round)
// ---------------------------------------------------------------------------

fn open_intent(dir: &Path, job: &str) {
    board::append_bid(
        dir,
        "astro-site",
        job,
        1,
        BidEntry::Intent {
            operation_id: "op".into(),
            amount: "2.00".into(),
            sla_seconds: 86_400,
            proposal_sha256: "aa".into(),
        },
        &clock(),
    )
    .unwrap();
}

fn pending_elsewhere(n: u32) -> Vec<MyBid> {
    (0..n)
        .map(|i| MyBid {
            bid_id: format!("old-{i}"),
            job_id: format!("other-{i}"),
            status: "pending".into(),
            amount: "2.00".into(),
        })
        .collect()
}

/// A resent intent that the market takes is a live bid: it stays counted
/// against `max_open_bids` for the rest of the pass.
#[tokio::test]
async fn a_resent_intent_stays_counted_against_the_open_bid_cap() {
    let dir = tempfile::tempdir().unwrap();
    open_intent(dir.path(), &job_id(20));
    let board = FakeBoard {
        jobs: vec![job(21)],
        bids: Mutex::new(pending_elsewhere(2)),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    // The resend went out; 2 pending + the resent bid = the cap of 3.
    let placed: Vec<String> = board
        .placed
        .lock()
        .unwrap()
        .iter()
        .map(|p| p.0.clone())
        .collect();
    assert_eq!(placed, vec![job_id(20)], "{ev:?}");
    assert_eq!(skipped(&ev, &job_id(21)).as_deref(), Some("max_open_bids"));
}

/// 429 and 408 say "not now": the intent stays open and the next pass
/// sends the bid again; it is never journaled as refused.
#[tokio::test]
async fn a_throttled_bid_stays_open_and_is_sent_again() {
    for status in [429u16, 408] {
        let dir = tempfile::tempdir().unwrap();
        let board = FakeBoard {
            jobs: vec![job(22)],
            answers: Mutex::new(vec![Err(MarketError::Refused {
                status,
                code: "rate_limited".into(),
                detail: "slow down".into(),
            })]),
            ..Default::default()
        };
        let runner = ScopeRunner::new(true);
        let mut state = BidderState::default();
        let ev = run_pass(
            &board,
            &runner,
            &ctx(false),
            &settings(),
            &mut state,
            dir.path(),
        )
        .await;
        assert!(
            !ev.iter().any(|e| matches!(e, BidEvent::Refused { .. })),
            "{ev:?}"
        );
        let path = board::bid_path(dir.path(), "astro-site", &job_id(22));
        assert_eq!(board::read_bid(&path).unwrap().unwrap().state(), "placing");
        run_pass(
            &board,
            &runner,
            &ctx(false),
            &settings(),
            &mut state,
            dir.path(),
        )
        .await;
        assert_eq!(board.placed.lock().unwrap().len(), 2, "{status}");
        assert_eq!(board::read_bid(&path).unwrap().unwrap().state(), "placed");
    }
}

/// At the cap no scope question is asked and no attachment is read.
#[tokio::test]
async fn a_full_listing_asks_no_scope_question() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(30), job(31), job(32)],
        bids: Mutex::new(pending_elsewhere(3)),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(*runner.asked.lock().unwrap(), 0, "{ev:?}");
    assert_eq!(*board.attachment_reads.lock().unwrap(), 0);
    assert_eq!(skipped(&ev, &job_id(30)).as_deref(), Some("max_open_bids"));
    assert!(board.placed.lock().unwrap().is_empty());
}

/// A passing scope answer is kept for the process: a job held back after
/// it (here the bid journal cannot be written) is not asked again.
#[cfg(unix)]
#[tokio::test]
async fn a_passing_scope_answer_is_not_asked_again() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bids = board::bids_dir(dir.path(), "astro-site");
    std::fs::create_dir_all(&bids).unwrap();
    std::fs::set_permissions(&bids, std::fs::Permissions::from_mode(0o500)).unwrap();
    let board = FakeBoard {
        jobs: vec![job(33)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    for _ in 0..2 {
        let ev = run_pass(
            &board,
            &runner,
            &ctx(true),
            &settings(),
            &mut state,
            dir.path(),
        )
        .await;
        assert!(
            ev.iter().any(|e| matches!(e, BidEvent::Error { .. })),
            "{ev:?}"
        );
    }
    assert_eq!(*runner.asked.lock().unwrap(), 1);
    std::fs::set_permissions(&bids, std::fs::Permissions::from_mode(0o700)).unwrap();
    run_pass(
        &board,
        &runner,
        &ctx(true),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(board.placed.lock().unwrap().len(), 1);
    assert_eq!(*runner.asked.lock().unwrap(), 1);
}

async fn pass_with_deadline(
    board: &FakeBoard,
    runner: &ScopeRunner,
    dir: &Path,
    deadline: std::time::Instant,
) -> Vec<BidEvent> {
    let (c, s) = (ctx(true), settings());
    let l = BidListing {
        ctx: &c,
        settings: &s,
        owner_verification: Verification::None,
    };
    let mut state = BidderState::default();
    board::pass(
        board,
        runner,
        &l,
        &mut state,
        dir,
        &Limits::default(),
        &clock(),
        PassInputs {
            assigned_jobs: None,
            deadline: Some(deadline),
        },
    )
    .await
}

/// A spent time budget stops the pass before it reads or bids anything.
#[tokio::test]
async fn a_spent_time_budget_stops_the_pass() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(34)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let ev = pass_with_deadline(&board, &runner, dir.path(), std::time::Instant::now()).await;
    assert!(
        ev.iter()
            .any(|e| e.to_string().contains(board::BUDGET_SPENT)),
        "{ev:?}"
    );
    assert_eq!(*board.pending_reads.lock().unwrap(), 0);
    assert!(board.placed.lock().unwrap().is_empty());
}

/// A slow scope answer is cut at the budget, not at the scope limit.
#[tokio::test]
async fn a_scope_question_waits_no_longer_than_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(35)],
        ..Default::default()
    };
    let mut runner = ScopeRunner::new(true);
    runner.delay = std::time::Duration::from_secs(30);
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_millis(300);
    let ev = pass_with_deadline(&board, &runner, dir.path(), deadline).await;
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{ev:?}"
    );
    assert!(board.placed.lock().unwrap().is_empty(), "{ev:?}");
}

/// The job's attachments reach the hard limits; a job whose attachments
/// cannot be read gets no bid and is read again on the next pass.
#[tokio::test]
async fn the_hard_limits_see_the_jobs_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = ctx(false);
    c.limits = Some(nearbee_core::listing::ListingLimits {
        max_attachments: Some(1),
        ..Default::default()
    });
    let file = |n: u32| Attachment {
        id: format!("00000000-0000-4000-8000-{n:012}"),
        filename: format!("f{n}.txt"),
        content_type: "text/plain".into(),
        byte_size: 10,
        sha256: "aa".into(),
        download_url: "/x".into(),
    };
    let board = FakeBoard {
        jobs: vec![job(36), job(37)],
        attachments: [(job_id(36), vec![file(1), file(2)])].into_iter().collect(),
        attachments_fail: [job_id(37)].into_iter().collect(),
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(&board, &runner, &c, &settings(), &mut state, dir.path()).await;
    assert_eq!(skipped(&ev, &job_id(36)).as_deref(), Some("hard_limits"));
    assert_eq!(
        skipped(&ev, &job_id(37)).as_deref(),
        Some("attachments_unreadable (403 forbidden)")
    );
    assert!(board.placed.lock().unwrap().is_empty());
    let ev = run_pass(&board, &runner, &c, &settings(), &mut state, dir.path()).await;
    assert!(skipped(&ev, &job_id(37)).is_some(), "{ev:?}");
}

/// A bid that leaves the pending list settles from the assignments: an
/// assignment on its job is the award, none closes it; with no assignment
/// read this pass nothing is written.
#[tokio::test]
async fn a_bid_off_the_pending_list_settles_from_the_assignments() {
    let dir = tempfile::tempdir().unwrap();
    let board = FakeBoard {
        jobs: vec![job(38), job(39)],
        ..Default::default()
    };
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    let path = |n| board::bid_path(dir.path(), "astro-site", &job_id(n));
    for b in board.bids.lock().unwrap().iter_mut() {
        b.status = if b.job_id == job_id(38) {
            "accepted"
        } else {
            "rejected"
        }
        .into();
    }
    let before = board::read_bid(&path(38)).unwrap().unwrap().records;
    run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert_eq!(board::read_bid(&path(38)).unwrap().unwrap().records, before);
    *board.assigned.lock().unwrap() = Some([job_id(38)].into_iter().collect());
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(
        ev.iter().any(|e| matches!(e, BidEvent::Awarded { job, bid_id, .. } if *job == job_id(38) && *bid_id == format!("bid-{}", job_id(38)))),
        "{ev:?}"
    );
    assert_eq!(
        board::read_bid(&path(38)).unwrap().unwrap().state(),
        "accepted"
    );
    assert_eq!(
        board::read_bid(&path(39)).unwrap().unwrap().state(),
        "closed"
    );
    assert_eq!(board.placed.lock().unwrap().len(), 2);
}

/// An intent left open whose bid was awarded before the crash closes as
/// placed and accepted, without a second bid.
#[tokio::test]
async fn an_open_intent_awarded_meanwhile_is_not_resent() {
    let dir = tempfile::tempdir().unwrap();
    open_intent(dir.path(), &job_id(40));
    let board = FakeBoard::default();
    *board.assigned.lock().unwrap() = Some([job_id(40)].into_iter().collect());
    let runner = ScopeRunner::new(true);
    let mut state = BidderState::default();
    let ev = run_pass(
        &board,
        &runner,
        &ctx(false),
        &settings(),
        &mut state,
        dir.path(),
    )
    .await;
    assert!(board.placed.lock().unwrap().is_empty(), "{ev:?}");
    let s = board::read_bid(&board::bid_path(dir.path(), "astro-site", &job_id(40)))
        .unwrap()
        .unwrap();
    assert_eq!(s.outcome.as_deref(), Some("placed"));
    assert_eq!(s.state(), "accepted");
}
