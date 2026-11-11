//! The runner seam: what Nearbee needs from a playbook engine. Implemented by
//! `nearbee-apb`; the core depends only on this trait and these types.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The facts an inspection reports; the type lives in
/// [`crate::tiers`] with the check that consumes it.
pub use crate::tiers::ExecutionFacts;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(v: impl Into<String>) -> Self {
                Self(v.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// The engine's id of the project Nearbee is pinned to.
    WorkspaceId
);
id_type!(
    /// A playbook id within the workspace.
    PlaybookId
);
id_type!(
    /// A run id assigned by the engine.
    RunId
);
id_type!(
    /// The market's hire id.
    HireId
);
id_type!(
    /// Nearbee's id of one execution of a hire (a hire may run more than once).
    ExecutionId
);

/// Param names the runner sets itself; a caller may not pass them.
pub const RESERVED_PARAMS: [&str; 3] = ["hire_id", "execution_id", "direct_models"];

/// A playbook as listed by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookSummary {
    pub id: PlaybookId,
    pub name: String,
    pub version: Option<String>,
}

/// An estimate of a run from the engine's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estimate {
    /// Median cost in USD when the engine has one.
    pub cost_usd: Option<Decimal>,
    /// Median total tokens when known.
    pub tokens: Option<u64>,
    /// Median duration in seconds when known.
    pub duration_secs: Option<Decimal>,
    /// How many comparable runs the estimate is based on.
    pub samples: u64,
    /// The engine's text summary.
    pub summary: Option<String>,
}

/// What to do when the engine has no estimate for a budgeted run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnNoEstimate {
    Ask,
    Continue,
    Pause,
}

impl OnNoEstimate {
    pub fn as_str(self) -> &'static str {
        match self {
            OnNoEstimate::Ask => "ask",
            OnNoEstimate::Continue => "continue",
            OnNoEstimate::Pause => "pause",
        }
    }
}

/// A run's budget. Money is a `Decimal`; the conversion to the engine's
/// wire format happens only at the adapter boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub usd: Option<Decimal>,
    pub on_no_estimate: OnNoEstimate,
}

/// A run start. `hire_id` and `execution_id` cannot be omitted; the runner
/// adds them to `params` and refuses a caller param with either name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRequest {
    pub playbook: PlaybookId,
    pub instruction: String,
    pub params: BTreeMap<String, String>,
    pub worktree: PathBuf,
    pub hire_id: HireId,
    pub execution_id: ExecutionId,
    /// The run's budget; `None` sends no budget object at all (the engine
    /// then runs unprotected).
    pub budget: Option<Budget>,
    /// The run this one continues (a rework or a redo); the engine sees
    /// the earlier run's context.
    pub continued_from: Option<RunId>,
    /// The generation's frozen direct models (design 3), sent as the
    /// `direct_models` param when not empty.
    pub direct_models: Vec<String>,
    /// Start the run as an apb external-data run (S6 design 3): serve sets
    /// it on every market run; callers that carry no buyer data do not.
    pub external_data: bool,
}

/// The model identity of one attempt, as apb reported it (design 7). A
/// missing reported model stays empty: it is never filled from the
/// requested one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reported {
    /// No task of the attempt reported a model.
    None,
    /// Every task reported a model and all name the same one.
    One(String),
    /// Reported models differ, or only some tasks reported one.
    Mixed,
}

/// One attempt of a run, with the models it was asked for and reported
/// (spec `apb-runner`, Requested models per attempt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptModel {
    pub node: String,
    pub attempt: u32,
    pub agent: String,
    pub requested_model: Option<String>,
    pub reported: Reported,
}

/// A run's status. Unknown statuses are kept and are neither terminal nor
/// successful.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    Created,
    Running,
    Paused,
    Interrupted,
    Succeeded,
    Failed,
    Aborted,
    Unknown(String),
}

impl RunStatus {
    /// Maps the engine's status word.
    pub fn parse(s: &str) -> Self {
        match s {
            "created" => Self::Created,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "interrupted" => Self::Interrupted,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "aborted" => Self::Aborted,
            other => Self::Unknown(other.to_owned()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Interrupted => "interrupted",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
            Self::Unknown(s) => s,
        }
    }

    /// The run will not change any more.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Aborted)
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded)
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One node of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeState {
    pub id: String,
    pub status: String,
    /// The node's outputs as reported by the engine, unparsed.
    pub outputs: Option<serde_json::Value>,
}

/// Reported costs split by their basis: each amount is `None` when no
/// attempt reported a figure on that basis (apb's `CostByBasis`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostByBasis {
    /// Money billed.
    pub billed_usd: Option<Decimal>,
    /// The list price of work a subscription covers; nobody is billed it.
    pub subscription_equivalent_usd: Option<Decimal>,
    /// Figures whose basis is not known.
    pub unknown_basis_usd: Option<Decimal>,
}

/// Usage of a run. A missing cost is `None` (unknown), never zero. The
/// counters are `None` when the engine named none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<Decimal>,
    /// The cost split by basis, when the engine reports one.
    pub cost_by_basis: Option<CostByBasis>,
    /// Attempts whose output reported usage.
    pub attempts: Option<u32>,
    /// Every attempt the run finished, with or without usage.
    pub finished_attempts: Option<u32>,
    /// How many of `attempts` reported a cost.
    pub cost_attempts: Option<u32>,
}

impl Usage {
    /// Whether the reported cost covers every finished attempt: `None`
    /// when no cost is reported or the counters are missing, `Some(false)`
    /// when fewer attempts reported a cost than the run finished.
    pub fn cost_complete(&self) -> Option<bool> {
        self.cost_usd?;
        match (self.cost_attempts, self.finished_attempts) {
            (Some(a), Some(f)) => Some(a == f),
            _ => None,
        }
    }
}

/// A run as observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDetail {
    pub id: RunId,
    pub playbook: PlaybookId,
    pub playbook_version: Option<String>,
    pub status: RunStatus,
    pub nodes: Vec<NodeState>,
    pub worktree: Option<PathBuf>,
    pub params: BTreeMap<String, String>,
    pub usage: Usage,
    /// The run's budget gate holds it (it is paused for its budget).
    pub budget_paused: bool,
    /// Per-attempt model records; empty when the engine gave none.
    pub attempts: Vec<AttemptModel>,
    /// The run reports itself an external-data run; false when the detail
    /// has no such field (S6 design 3.4).
    pub external_data: bool,
}

/// A run lookup by search field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunQuery {
    pub playbook: Option<PlaybookId>,
    pub field: SearchField,
}

/// The search fields Nearbee sets on every run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchField {
    HireId(HireId),
    ExecutionId(ExecutionId),
}

impl SearchField {
    pub fn name(&self) -> &'static str {
        match self {
            Self::HireId(_) => "hire_id",
            Self::ExecutionId(_) => "execution_id",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::HireId(v) => v.as_str(),
            Self::ExecutionId(v) => v.as_str(),
        }
    }
}

/// A run found by a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub id: RunId,
    pub playbook: PlaybookId,
    pub status: RunStatus,
}

/// The result of a lookup. `complete` is false when the engine withheld runs
/// or could not search some of them; then an empty `runs` does not prove
/// that no run exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLookup {
    pub runs: Vec<RunSummary>,
    pub complete: bool,
    pub withheld: u64,
    pub not_indexed: u64,
}

/// A runner failure. Every `detail` is bounded and redacted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunnerError {
    /// A known pre-start gate refused (budget, launch confirmation, invalid
    /// params, ...). The runner never answers such a gate itself.
    #[error("refused ({status} {code}): {detail}")]
    Refused {
        status: u16,
        code: String,
        detail: String,
        /// The engine's estimate when the refusal carried one.
        estimate: Option<serde_json::Value>,
    },
    /// A read failed on transport or a retryable status after its retries.
    #[error("read failed: {detail}")]
    RetryableRead { detail: String },
    /// A start was sent and its outcome is unknown (timeout, reset, server
    /// error, malformed success). It is never retried by the runner.
    #[error("start outcome unknown: {detail}")]
    StartOutcomeUnknown { detail: String },
    /// An answer the runner cannot understand.
    #[error("protocol error: {detail}")]
    Protocol { detail: String },
    /// A caller error caught before anything was sent.
    #[error("invalid request: {detail}")]
    Invalid { detail: String },
    /// A requested object does not exist (404).
    #[error("not found: {detail}")]
    NotFound { detail: String },
}

/// A yes or no decision from the engine's decision provider, asked
/// outside any run (the intake scope check).
#[derive(Debug, Clone, PartialEq)]
pub enum DecideOutcome {
    /// Answered: `is` answers yes or no, with the probability of yes.
    Answered { yes: bool, p: Option<Decimal> },
    /// No answer: apb's refusal code (`no_provider`, ...), `error` for a
    /// provider failure, or `timeout`.
    Unanswered { reason: String },
}

/// How long an abort or a decide may take before the child is killed.
pub const CLI_ABORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
pub const CLI_DECIDE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// What Nearbee needs from a playbook engine pinned to one workspace.
pub trait Runner: Send + Sync {
    fn workspace(&self) -> &WorkspaceId;
    fn playbooks(&self) -> impl Future<Output = Result<Vec<PlaybookSummary>, RunnerError>> + Send;
    fn estimate(
        &self,
        id: &PlaybookId,
    ) -> impl Future<Output = Result<Estimate, RunnerError>> + Send;
    fn start(&self, req: StartRequest) -> impl Future<Output = Result<RunId, RunnerError>> + Send;
    /// Whether the engine takes external-data runs (S6 design 3.2): one
    /// health read, no retry beyond the read policy. The version knowledge
    /// stays in the adapter; the core sees a bool.
    fn external_data_supported(&self) -> impl Future<Output = Result<bool, RunnerError>> + Send;
    fn run(&self, id: &RunId) -> impl Future<Output = Result<RunDetail, RunnerError>> + Send;
    /// Stops a running run (apb's pause: a predictive stop before agent
    /// nodes). Best effort; serve ignores a failure and reports the hire.
    fn stop(&self, id: &RunId) -> impl Future<Output = Result<(), RunnerError>> + Send;
    /// Aborts a run: no further node starts, in-flight agent work is
    /// killed. Success means the command exited 0; the caller confirms the
    /// terminal status by observation (an abort of an already terminal run
    /// is a success or a known "not running" answer).
    fn abort(&self, id: &RunId) -> impl Future<Output = Result<(), RunnerError>> + Send;
    /// Asks a yes or no question outside any run (no shell; the child
    /// carries no run or node environment). Answered, refused, or an error.
    /// `limit` is the longest the caller can wait: the engine's own child
    /// is killed and reaped by then (the smaller of `limit` and its own
    /// timeout), so a cut-off question leaves nothing running (R3-9).
    fn decide(
        &self,
        question: &str,
        criteria: &str,
        limit: std::time::Duration,
    ) -> impl Future<Output = Result<DecideOutcome, RunnerError>> + Send;
    fn find(&self, q: RunQuery) -> impl Future<Output = Result<RunLookup, RunnerError>> + Send;
    /// The execution facts of an installed playbook: its profile
    /// references and nodes from the engine's playbook detail, the named
    /// profiles from the profile detail routes, the provider routes and
    /// the project configuration from the workspace files. Every fact the
    /// engine or the files do not provide is reported unknown with the
    /// reason, never guessed (spec `apb-runner`, Playbook inspection).
    fn inspect(
        &self,
        id: &PlaybookId,
    ) -> impl Future<Output = Result<ExecutionFacts, RunnerError>> + Send;
    /// The same facts for a playbook YAML that is not installed: the
    /// playbook structure comes from the given source, everything else as
    /// in [`Runner::inspect`].
    fn inspect_source(
        &self,
        source: &str,
    ) -> impl Future<Output = Result<ExecutionFacts, RunnerError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_covers_every_spec_status() {
        for (s, terminal, success) in [
            ("created", false, false),
            ("running", false, false),
            ("paused", false, false),
            ("interrupted", false, false),
            ("succeeded", true, true),
            ("failed", true, false),
            ("aborted", true, false),
        ] {
            let st = RunStatus::parse(s);
            assert!(!matches!(st, RunStatus::Unknown(_)), "{s}");
            assert_eq!(st.is_terminal(), terminal, "{s}");
            assert_eq!(st.is_success(), success, "{s}");
            assert_eq!(st.as_str(), s);
        }
    }

    /// Cost completeness: missing cost stays unknown, a partial cost is
    /// incomplete, a full cost complete (spec `apb-runner`, Run
    /// observation).
    #[test]
    fn cost_completeness_follows_the_counters() {
        let full = Usage {
            cost_usd: Some(Decimal::new(5, 2)),
            cost_attempts: Some(3),
            finished_attempts: Some(3),
            ..Default::default()
        };
        assert_eq!(full.cost_complete(), Some(true));
        let partial = Usage {
            cost_usd: Some(Decimal::new(5, 2)),
            cost_attempts: Some(1),
            finished_attempts: Some(3),
            ..Default::default()
        };
        assert_eq!(partial.cost_complete(), Some(false));
        let missing = Usage::default();
        assert_eq!(missing.cost_complete(), None, "no cost: unknown");
        let uncounted = Usage {
            cost_usd: Some(Decimal::new(5, 2)),
            ..Default::default()
        };
        assert_eq!(uncounted.cost_complete(), None, "no counters: unknown");
    }

    #[test]
    fn unknown_status_is_neither_terminal_nor_success() {
        let st = RunStatus::parse("archived_by_martians");
        assert_eq!(st, RunStatus::Unknown("archived_by_martians".into()));
        assert!(!st.is_terminal());
        assert!(!st.is_success());
    }
}
