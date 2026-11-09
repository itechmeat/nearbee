//! The runner seam: what Nearbee needs from a playbook engine. Implemented by
//! `nearbee-apb`; the core depends only on this trait and these types.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;

use rust_decimal::Decimal;

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
pub const RESERVED_PARAMS: [&str; 2] = ["hire_id", "execution_id"];

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
    pub budget: Budget,
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

/// Usage of a run. A missing cost is `None` (unknown), never zero.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<Decimal>,
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

/// What Nearbee needs from a playbook engine pinned to one workspace.
pub trait Runner: Send + Sync {
    fn workspace(&self) -> &WorkspaceId;
    fn playbooks(&self) -> impl Future<Output = Result<Vec<PlaybookSummary>, RunnerError>> + Send;
    fn estimate(
        &self,
        id: &PlaybookId,
    ) -> impl Future<Output = Result<Estimate, RunnerError>> + Send;
    fn start(&self, req: StartRequest) -> impl Future<Output = Result<RunId, RunnerError>> + Send;
    fn run(&self, id: &RunId) -> impl Future<Output = Result<RunDetail, RunnerError>> + Send;
    fn find(&self, q: RunQuery) -> impl Future<Output = Result<RunLookup, RunnerError>> + Send;
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

    #[test]
    fn unknown_status_is_neither_terminal_nor_success() {
        let st = RunStatus::parse("archived_by_martians");
        assert_eq!(st, RunStatus::Unknown("archived_by_martians".into()));
        assert!(!st.is_terminal());
        assert!(!st.is_success());
    }
}
