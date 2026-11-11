//! The runner adapter: drives a local `apb dashboard` over its REST API,
//! pinned to one apb workspace (project).
//!
//! Everything that knows apb lives in this crate behind
//! [`nearbee_core::Runner`]. Field names follow fixtures captured from the
//! installed apb (`tests/fixtures/apb/`), not apb's source tree.
//!
//! Calls: reads (`GET`) are retried on transport errors and on 429, 502, 503
//! and 504 (one try plus up to three retries, 0.5 s, 1 s, 2 s apart); a run
//! start is sent exactly once and never retried; an answer to it that is
//! neither a success nor a 4xx refusal leaves its outcome unknown. Redirects are not followed
//! and system proxies are not used. Every error detail is cut to 2 KiB and
//! has configured secret values and bearer tokens removed.

pub mod cli;
mod inspect;
pub mod install;
pub mod run_params;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nearbee_core::Secret;
use nearbee_core::redact::redact_detail;
use nearbee_core::runner::{
    AttemptModel, CostByBasis, DecideOutcome, Estimate, ExecutionFacts, NodeState, PlaybookId,
    PlaybookSummary, RESERVED_PARAMS, Reported, RunDetail, RunId, RunLookup, RunQuery, RunStatus,
    RunSummary, Runner, RunnerError, StartRequest, Usage, WorkspaceId,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use serde_json::Value;

/// Timeouts and retry pacing of the client.
#[derive(Debug, Clone)]
pub struct Timing {
    pub connect: Duration,
    pub read: Duration,
    pub estimate: Duration,
    pub start: Duration,
    /// How long one `apb stop` may take before the child is killed.
    pub abort: Duration,
    /// How long one `apb decide` may take before the child is killed.
    pub decide: Duration,
    /// Delay before each retry of a read; its length is the number of retries.
    pub read_retry_delays: Vec<Duration>,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(3),
            read: Duration::from_secs(10),
            estimate: Duration::from_secs(30),
            start: Duration::from_secs(30),
            abort: nearbee_core::runner::CLI_ABORT_TIMEOUT,
            decide: nearbee_core::runner::CLI_DECIDE_TIMEOUT,
            read_retry_delays: vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
            ],
        }
    }
}

/// How to reach apb.
#[derive(Debug, Clone)]
pub struct ApbOptions {
    pub url: String,
    pub api_key: Option<Secret>,
    /// Every secret value to remove from error details (the API key is added
    /// automatically).
    pub redact: Vec<Secret>,
    pub timing: Timing,
    /// The apb CLI binary, for the CLI calls (abort, decide).
    pub bin: PathBuf,
    /// The `APB_CONFIG_DIR` of Nearbee's own apb instance, for the CLI
    /// calls. Never the operator's everyday instance.
    pub config_dir: PathBuf,
    /// The `systemctl` binary the inspection reads the instance
    /// environment with (`systemctl --user show nearbee-apb -p
    /// Environment`).
    pub systemctl_bin: PathBuf,
    /// The `XDG_CONFIG_HOME` the instance's unit must carry: Nearbee's own
    /// directory with no OpenCode configuration in it (the inspection
    /// makes the model routes unknown otherwise).
    pub xdg_config_home: PathBuf,
}

/// One apb project as listed by the dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub path: PathBuf,
}

/// The configured workspace is not among apb's projects.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("apb has no project at {path}; known projects: {known}", path = .path.display(), known = known_list(.known))]
pub struct WorkspaceNotFound {
    pub path: PathBuf,
    pub known: Vec<PathBuf>,
}

fn known_list(known: &[PathBuf]) -> String {
    if known.is_empty() {
        return "none".into();
    }
    known
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Finds the project whose path is the configured workspace path. Both sides
/// are canonicalized when they exist, so a symlinked or `..` spelling of the
/// same directory still matches.
pub fn resolve_workspace(
    projects: &[Project],
    workspace_path: &Path,
) -> Result<WorkspaceId, WorkspaceNotFound> {
    let want = canonical(workspace_path);
    projects
        .iter()
        .find(|p| canonical(&p.path) == want)
        .map(|p| p.workspace_id.clone())
        .ok_or_else(|| WorkspaceNotFound {
            path: workspace_path.to_path_buf(),
            known: projects.iter().map(|p| p.path.clone()).collect(),
        })
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The transport settings of every request to apb: no redirects, no system
/// proxy, a connect timeout, and no transport-level retries. reqwest's
/// default policy silently resends a request (body included) on an HTTP/2
/// `REFUSED_STREAM` or graceful `GOAWAY`; the start request must reach apb
/// at most once per call, and read retries are this crate's own.
fn http_client_builder(connect: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(connect)
}

/// The HTTP client for one apb dashboard.
#[derive(Debug, Clone)]
pub struct ApbClient {
    http: reqwest::Client,
    base: String,
    key: Option<Secret>,
    secrets: Vec<Secret>,
    timing: Timing,
}

pub(crate) enum Kind {
    Read,
    Estimate,
}

impl ApbClient {
    pub fn new(opts: ApbOptions) -> Result<Self, RunnerError> {
        let mut secrets = opts.redact;
        if let Some(k) = &opts.api_key {
            secrets.push(k.clone());
        }
        let http = http_client_builder(opts.timing.connect)
            .build()
            .map_err(|e| RunnerError::Invalid {
                detail: redact_detail(&e.to_string(), &secrets),
            })?;
        let base = opts.url.trim_end_matches('/').to_owned();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(RunnerError::Invalid {
                detail: format!("apb URL must start with http:// or https://: {base}"),
            });
        }
        Ok(Self {
            http,
            base,
            key: opts.api_key,
            secrets,
            timing: opts.timing,
        })
    }

    fn redact(&self, text: &str) -> String {
        redact_detail(text, &self.secrets)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut rb = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(k) = &self.key {
            rb = rb.bearer_auth(k.expose());
        }
        rb
    }

    /// A `GET` with retries. 404 is `NotFound`; any other non-success status
    /// that is not retryable is a protocol error.
    async fn get(
        &self,
        path: &str,
        query: &[(&str, &str)],
        kind: Kind,
    ) -> Result<Value, RunnerError> {
        let timeout = match kind {
            Kind::Read => self.timing.read,
            Kind::Estimate => self.timing.estimate,
        };
        let mut delays = self.timing.read_retry_delays.iter();
        loop {
            let res = self
                .request(reqwest::Method::GET, path)
                .query(query)
                .timeout(timeout)
                .send()
                .await;
            let retry_detail = match res {
                Err(e) => format!("GET {path}: {}", describe(&e)),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if matches!(status, 429 | 502 | 503 | 504) {
                        format!("GET {path}: HTTP {status}")
                    } else {
                        match resp.text().await {
                            // The status alone says the object is unknown:
                            // a 404 is NotFound and sent once, even when its
                            // body cannot be read.
                            Err(_) if status == 404 => {
                                return Err(RunnerError::NotFound {
                                    detail: self.redact(&format!("GET {path}: HTTP 404")),
                                });
                            }
                            // Any other transport failure while reading the
                            // body is retried like one before the headers.
                            Err(e) => format!("GET {path}: HTTP {status}: {}", describe(&e)),
                            Ok(text) => return self.answer(path, status, &text),
                        }
                    }
                }
            };
            match delays.next() {
                Some(d) => {
                    tracing::debug!(detail = %self.redact(&retry_detail), "retrying apb read");
                    tokio::time::sleep(*d).await;
                }
                None => {
                    return Err(RunnerError::RetryableRead {
                        detail: self.redact(&retry_detail),
                    });
                }
            }
        }
    }

    /// The answer to a `GET` whose body was read: 404 is `NotFound`, any
    /// other non-success status a protocol error, a success must be JSON.
    fn answer(&self, path: &str, status: u16, text: &str) -> Result<Value, RunnerError> {
        if status == 404 {
            return Err(RunnerError::NotFound {
                detail: self.redact(&format!("GET {path}: {}", text.trim())),
            });
        }
        if !(200..300).contains(&status) {
            return Err(RunnerError::Protocol {
                detail: self.redact(&format!("GET {path}: HTTP {status}: {}", text.trim())),
            });
        }
        serde_json::from_str(text).map_err(|e| RunnerError::Protocol {
            detail: self.redact(&format!("GET {path}: not JSON: {e}")),
        })
    }

    /// `GET /api/projects`.
    pub async fn projects(&self) -> Result<Vec<Project>, RunnerError> {
        let v = self.get("/api/projects", &[], Kind::Read).await?;
        let arr = v
            .as_array()
            .ok_or_else(|| self.protocol("/api/projects: not a list"))?;
        arr.iter()
            .map(|p| {
                Ok(Project {
                    workspace_id: WorkspaceId::new(str_field(p, "workspace_id").ok_or_else(
                        || self.protocol("/api/projects: entry without workspace_id"),
                    )?),
                    name: str_field(p, "name").unwrap_or_default(),
                    path: PathBuf::from(
                        str_field(p, "path")
                            .ok_or_else(|| self.protocol("/api/projects: entry without path"))?,
                    ),
                })
            })
            .collect()
    }

    fn protocol(&self, detail: &str) -> RunnerError {
        RunnerError::Protocol {
            detail: self.redact(detail),
        }
    }
}

fn describe(e: &reqwest::Error) -> String {
    let what = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "connection failed"
    } else if e.is_body() || e.is_decode() {
        "reading the answer failed"
    } else {
        "request failed"
    };
    // reqwest's own text may include the URL; it never includes headers.
    format!("{what}: {e}")
}

pub(crate) fn str_field(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn decimal(v: &Value) -> Option<Decimal> {
    match v {
        Value::Number(n) => n
            .as_f64()
            .and_then(Decimal::from_f64)
            .or_else(|| n.as_i64().map(Decimal::from)),
        _ => None,
    }
}

/// [`Runner`] over a local apb dashboard, pinned to one workspace.
#[derive(Debug, Clone)]
pub struct ApbRunner {
    client: ApbClient,
    workspace: WorkspaceId,
    /// The workspace path: the working directory of the CLI calls.
    workspace_path: PathBuf,
    /// The CLI binary and the instance's config directory.
    cli_bin: PathBuf,
    cli_config_dir: PathBuf,
    /// The `systemctl` binary the inspection reads the instance
    /// environment with.
    systemctl_bin: PathBuf,
    /// The instance's expected `XDG_CONFIG_HOME`.
    xdg_config_home: PathBuf,
}

/// Why [`ApbRunner::connect`] failed.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceNotFound),
}

impl ApbRunner {
    /// Reads apb's project list once and pins the runner to the project at
    /// `workspace_path`.
    pub async fn connect(opts: ApbOptions, workspace_path: &Path) -> Result<Self, ConnectError> {
        let cli_bin = opts.bin.clone();
        let cli_config_dir = opts.config_dir.clone();
        let systemctl_bin = opts.systemctl_bin.clone();
        let xdg_config_home = opts.xdg_config_home.clone();
        let client = ApbClient::new(opts)?;
        let projects = client.projects().await?;
        let workspace = resolve_workspace(&projects, workspace_path)?;
        Ok(Self {
            client,
            workspace,
            workspace_path: workspace_path.to_path_buf(),
            cli_bin,
            cli_config_dir,
            systemctl_bin,
            xdg_config_home,
        })
    }

    /// Runs the CLI on the blocking thread pool: a child process must not
    /// stall the async runtime.
    async fn cli(
        &self,
        step: &'static str,
        args: Vec<String>,
        remove: &[&str],
        timeout: std::time::Duration,
    ) -> Result<cli::CliRun, RunnerError> {
        let bin = self.cli_bin.clone();
        let workspace = self.workspace_path.clone();
        let config_dir = self.cli_config_dir.clone();
        let owned: Vec<String> = args;
        let remove_owned: Vec<String> = remove.iter().map(|s| (*s).to_owned()).collect();
        tokio::task::spawn_blocking(move || {
            let borrowed: Vec<&str> = owned.iter().map(String::as_str).collect();
            let removed: Vec<&str> = remove_owned.iter().map(String::as_str).collect();
            cli::run(&bin, &borrowed, &workspace, &config_dir, &removed, timeout).map_err(|e| {
                RunnerError::Protocol {
                    detail: format!("apb {step}: {e}"),
                }
            })
        })
        .await
        .map_err(|e| RunnerError::Protocol {
            detail: format!("apb {step}: the cli task failed: {e}"),
        })?
    }

    fn ws(&self) -> [(&str, &str); 1] {
        [("workspace", self.workspace.as_str())]
    }

    /// Builds the start body, refusing caller errors before anything is sent.
    fn start_body(&self, req: &StartRequest) -> Result<Value, RunnerError> {
        for name in RESERVED_PARAMS {
            if req.params.contains_key(name) {
                return Err(RunnerError::Invalid {
                    detail: format!(
                        "param `{name}` is reserved: the runner sets it from the request"
                    ),
                });
            }
        }
        let meta = std::fs::metadata(&req.worktree).map_err(|_| RunnerError::Invalid {
            detail: format!("worktree {} does not exist", req.worktree.display()),
        })?;
        if !meta.is_dir() {
            return Err(RunnerError::Invalid {
                detail: format!("worktree {} is not a directory", req.worktree.display()),
            });
        }
        let worktree = req
            .worktree
            .canonicalize()
            .map_err(|e| RunnerError::Invalid {
                detail: format!("worktree {}: {e}", req.worktree.display()),
            })?;
        let params = sent_params(req);
        // No budget, no budget key: the engine then runs unprotected
        // (spec `apb-runner`, Run start, scenario "No budget").
        let budget = match req.budget.as_ref() {
            Some(b) => {
                let mut budget = serde_json::Map::new();
                if let Some(usd) = b.usd {
                    // apb takes dollars as a JSON float: the only place
                    // Decimal becomes a float.
                    let f = usd
                        .to_f64()
                        .filter(|f| f.is_finite() && *f > 0.0)
                        .ok_or_else(|| RunnerError::Invalid {
                            detail: format!("budget usd must be a positive amount, got {usd}"),
                        })?;
                    budget.insert("usd".into(), Value::from(f));
                }
                budget.insert(
                    "on_no_estimate".into(),
                    Value::from(b.on_no_estimate.as_str()),
                );
                Some(Value::Object(budget))
            }
            None => None,
        };
        let mut body = serde_json::Map::new();
        body.insert("instruction".into(), Value::from(req.instruction.clone()));
        body.insert(
            "params".into(),
            Value::Object(
                params
                    .into_iter()
                    .map(|(k, v)| (k, Value::from(v)))
                    .collect(),
            ),
        );
        body.insert(
            "worktree".into(),
            Value::from(worktree.to_string_lossy().to_string()),
        );
        if let Some(b) = budget {
            body.insert("budget".into(), b);
        }
        if let Some(from) = &req.continued_from {
            body.insert("continued_from".into(), Value::from(from.as_str()));
        }
        Ok(Value::Object(body))
    }
}

/// Longest refusal code kept, in bytes.
const CODE_LIMIT: usize = 128;
/// Largest refusal estimate kept, serialized, in bytes; a larger one is
/// dropped.
const ESTIMATE_LIMIT: usize = 8192;

/// Maps a refusal body (JSON or plain text) to [`RunnerError::Refused`].
/// Code, detail and every string inside the estimate are redacted and
/// bounded.
fn refused(status: u16, text: &str, redact: impl Fn(&str) -> String) -> RunnerError {
    let parsed: Option<Value> = serde_json::from_str(text).ok();
    let (code, detail, estimate) = match &parsed {
        Some(v @ Value::Object(_)) => {
            let code = ["error", "policy", "code"]
                .iter()
                .find_map(|k| str_field(v, k))
                .unwrap_or_else(|| format!("http_{status}"));
            let detail = ["detail", "instruction", "message"]
                .iter()
                .find_map(|k| str_field(v, k))
                .unwrap_or_else(|| v.to_string());
            (code, detail, v.get("estimate").cloned())
        }
        _ => (format!("http_{status}"), text.trim().to_owned(), None),
    };
    let estimate = estimate
        .map(|e| redact_strings(e, &redact))
        .filter(|e| e.to_string().len() <= ESTIMATE_LIMIT);
    RunnerError::Refused {
        status,
        code: nearbee_core::redact::truncate(&redact(&code), CODE_LIMIT),
        detail: redact(&detail),
        estimate,
    }
}

/// Redacts every string (keys included) inside a JSON value.
fn redact_strings(v: Value, redact: &impl Fn(&str) -> String) -> Value {
    match v {
        Value::String(s) => Value::String(redact(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| redact_strings(x, redact)).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, x)| (redact(&k), redact_strings(x, redact)))
                .collect(),
        ),
        other => other,
    }
}

/// Parses apb's reported model record of one attempt (`reported.kind`:
/// `none` gives no model, `one` gives its model, anything else is mixed;
/// design 7). A missing reported model stays empty.
pub fn parse_reported(v: &Value) -> Reported {
    match v.get("kind").and_then(Value::as_str) {
        Some("none") => Reported::None,
        Some("one") => match str_field(v, "model") {
            Some(model) => Reported::One(model),
            None => Reported::Mixed,
        },
        _ => Reported::Mixed,
    }
}

/// Parses the run's `attempt_models` (apb's `ExecutionIdentityView`).
fn parse_attempts(v: &Value) -> Vec<AttemptModel> {
    v.get("attempt_models")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some(AttemptModel {
                        node: str_field(r, "node")?,
                        attempt: r
                            .get("attempt")
                            .and_then(Value::as_u64)
                            .and_then(|a| u32::try_from(a).ok())?,
                        agent: str_field(r, "agent").unwrap_or_default(),
                        requested_model: str_field(r, "requested_model"),
                        reported: parse_reported(r.get("reported").unwrap_or(&Value::Null)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parses a run detail answer.
pub fn parse_run_detail(v: &Value) -> Result<RunDetail, String> {
    let id = str_field(v, "run_id").ok_or("run detail without run_id")?;
    let status = str_field(v, "run_status").ok_or("run detail without run_status")?;
    let playbook = str_field(v, "playbook").ok_or("run detail without playbook")?;
    let outputs = v.get("outputs").and_then(Value::as_object);
    let nodes = v
        .get("nodes")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(id, st)| NodeState {
                    id: id.clone(),
                    status: st.as_str().unwrap_or_default().to_owned(),
                    outputs: outputs.and_then(|o| o.get(id)).cloned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let params = v
        .get("params")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let usage = v.get("usage").filter(|u| u.is_object()).map(|u| Usage {
        input_tokens: u.get("input_tokens").and_then(Value::as_u64),
        output_tokens: u.get("output_tokens").and_then(Value::as_u64),
        cost_usd: u.get("cost_usd").and_then(decimal),
        cost_by_basis: u
            .get("cost_by_basis")
            .filter(|c| c.is_object())
            .map(|c| CostByBasis {
                billed_usd: c.get("billed_usd").and_then(decimal),
                subscription_equivalent_usd: c.get("subscription_equivalent_usd").and_then(decimal),
                unknown_basis_usd: c.get("unknown_basis_usd").and_then(decimal),
            }),
        attempts: u
            .get("attempts")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok()),
        finished_attempts: u
            .get("finished_attempts")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok()),
        cost_attempts: u
            .get("cost_attempts")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok()),
    });
    // The budget gate pause: `budget.paused` holds the run before a node
    // (apb's BudgetStatus at the tag).
    let budget_paused = v
        .get("budget")
        .filter(|b| b.is_object())
        .and_then(|b| b.get("paused"))
        .is_some_and(|p| p.is_object());
    Ok(RunDetail {
        id: RunId::new(id),
        playbook: PlaybookId::new(playbook),
        playbook_version: str_field(v, "version"),
        status: RunStatus::parse(&status),
        nodes,
        worktree: str_field(v, "worktree").map(PathBuf::from),
        params,
        usage: usage.unwrap_or_default(),
        budget_paused,
        attempts: parse_attempts(v),
    })
}

/// Parses an estimate answer.
pub fn parse_estimate(v: &Value) -> Estimate {
    let p50 = |x: &Value| -> Option<Value> {
        match x {
            Value::Object(m) => m
                .get("p50")
                .cloned()
                .or_else(|| m.get("spread").and_then(|s| s.get("p50")).cloned()),
            Value::Null => None,
            other => Some(other.clone()),
        }
    };
    Estimate {
        cost_usd: v.get("cost_usd").and_then(p50).as_ref().and_then(decimal),
        tokens: v.get("tokens").and_then(p50).and_then(|t| t.as_u64()),
        duration_secs: v
            .get("time")
            .and_then(p50)
            .and_then(|ms| decimal(&ms))
            .map(|ms| ms / Decimal::from(1000)),
        samples: v
            .get("comparable")
            .and_then(|c| c.get("runs"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        summary: str_field(v, "summary"),
    }
}

/// Parses a run search answer `{runs, not_indexed, withheld}`.
pub fn parse_lookup(v: &Value) -> Result<RunLookup, String> {
    let runs = v
        .get("runs")
        .and_then(Value::as_array)
        .ok_or("search answer without runs")?;
    let count = |k: &str| {
        v.get(k)
            .and_then(Value::as_u64)
            .ok_or(format!("search answer without {k}"))
    };
    let not_indexed = count("not_indexed")?;
    let withheld = count("withheld")?;
    let runs = runs
        .iter()
        .map(|r| {
            Ok(RunSummary {
                id: RunId::new(str_field(r, "run_id").ok_or("search row without run_id")?),
                playbook: PlaybookId::new(str_field(r, "playbook").unwrap_or_default()),
                status: RunStatus::parse(&str_field(r, "status").unwrap_or_default()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RunLookup {
        runs,
        complete: not_indexed == 0 && withheld == 0,
        withheld,
        not_indexed,
    })
}

impl Runner for ApbRunner {
    fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    async fn playbooks(&self) -> Result<Vec<PlaybookSummary>, RunnerError> {
        let v = self.client.get("/api/playbooks", &[], Kind::Read).await?;
        let arr = v
            .as_array()
            .ok_or_else(|| self.client.protocol("/api/playbooks: not a list"))?;
        Ok(arr
            .iter()
            .filter(|p| str_field(p, "workspace_id").as_deref() == Some(self.workspace.as_str()))
            .filter_map(|p| {
                Some(PlaybookSummary {
                    id: PlaybookId::new(str_field(p, "id")?),
                    name: str_field(p, "name").unwrap_or_default(),
                    version: str_field(p, "current"),
                })
            })
            .collect())
    }

    async fn estimate(&self, id: &PlaybookId) -> Result<Estimate, RunnerError> {
        let path = format!("/api/playbooks/{}/estimate", path_segment(id.as_str())?);
        let v = self.client.get(&path, &self.ws(), Kind::Estimate).await?;
        Ok(parse_estimate(&v))
    }

    async fn start(&self, req: StartRequest) -> Result<RunId, RunnerError> {
        let body = self.start_body(&req)?;
        let path = format!(
            "/api/playbooks/{}/run",
            path_segment(req.playbook.as_str())?
        );
        let unknown = |detail: String| RunnerError::StartOutcomeUnknown {
            detail: self.client.redact(&detail),
        };
        let resp = self
            .client
            .request(reqwest::Method::POST, &path)
            .query(&self.ws())
            .timeout(self.client.timing.start)
            .json(&body)
            .send()
            .await
            .map_err(|e| unknown(format!("POST {path}: {}", describe(&e))))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| unknown(format!("POST {path}: HTTP {status}: {}", describe(&e))))?;
        if (200..300).contains(&status) {
            let v: Value = serde_json::from_str(&text).map_err(|_| {
                unknown(format!(
                    "POST {path}: HTTP {status} with a body that is not JSON"
                ))
            })?;
            let id = str_field(&v, "run_id")
                .ok_or_else(|| unknown(format!("POST {path}: HTTP {status} without run_id")))?;
            // An id that cannot be observed later is no known start: the
            // caller must recover the run by its hire and execution ids.
            path_segment(&id).map_err(|_| {
                unknown(format!(
                    "POST {path}: HTTP {status} with an unusable run_id"
                ))
            })?;
            return Ok(RunId::new(id));
        }
        if status >= 500 {
            return Err(unknown(format!(
                "POST {path}: HTTP {status}: {}",
                text.trim()
            )));
        }
        // The start was sent and a redirect is not followed, so a 3xx (or
        // any other non-4xx answer) leaves its outcome unknown.
        if !(400..500).contains(&status) {
            return Err(unknown(format!(
                "POST {path}: unexpected HTTP {status} to a start"
            )));
        }
        Err(refused(status, &text, |s| self.client.redact(s)))
    }

    async fn run(&self, id: &RunId) -> Result<RunDetail, RunnerError> {
        let path = format!("/api/runs/{}", path_segment(id.as_str())?);
        let v = self.client.get(&path, &self.ws(), Kind::Read).await?;
        parse_run_detail(&v).map_err(|e| self.client.protocol(&format!("GET {path}: {e}")))
    }

    async fn stop(&self, id: &RunId) -> Result<(), RunnerError> {
        // apb has no hard cancel for a run; pause is its predictive stop:
        // no further agent node starts, the engine keeps the state.
        let path = format!("/api/runs/{}/pause", path_segment(id.as_str())?);
        let resp = self
            .client
            .request(reqwest::Method::POST, &path)
            .query(&self.ws())
            .timeout(self.client.timing.read)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| RunnerError::RetryableRead {
                detail: self
                    .client
                    .redact(&format!("POST {path}: {}", describe(&e))),
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| RunnerError::RetryableRead {
            detail: self
                .client
                .redact(&format!("POST {path}: {}", describe(&e))),
        })?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(RunnerError::Refused {
            status,
            code: String::new(),
            detail: self.client.redact(&text),
            estimate: None,
        })
    }

    /// `apb stop <run_id>`: posts the abort the driving process picks up
    /// (or finalizes a driverless run itself). Exit 0 is success, also for
    /// an already terminal run ("had already finished"); exit 2 names the
    /// failure. No retry inside the call; serve repeats it each round
    /// until the observation reads terminal.
    async fn abort(&self, id: &RunId) -> Result<(), RunnerError> {
        let run_id = path_segment(id.as_str())?.to_owned();
        let out = self
            .cli(
                "stop",
                vec!["stop".into(), run_id],
                &[],
                self.client.timing.abort,
            )
            .await?;
        if out.status == Some(0) {
            return Ok(());
        }
        Err(RunnerError::Refused {
            status: 0,
            code: "stop_failed".into(),
            detail: format!("apb stop: {}", [out.stdout, out.stderr].join(" ").trim()),
            estimate: None,
        })
    }

    /// `apb decide is <question> --criteria <criteria> --no-run`: one JSON
    /// line on stdout, exit 0 answered, 1 refused or failed, 2 malformed.
    /// The brief travels as one argv element; the child carries no
    /// `APB_RUN_ID` or `APB_NODE_ID`, so the decision is journaled outside
    /// any run.
    async fn decide(
        &self,
        question: &str,
        criteria: &str,
        limit: std::time::Duration,
    ) -> Result<DecideOutcome, RunnerError> {
        let out = self
            .cli(
                "decide",
                vec![
                    "decide".into(),
                    "is".into(),
                    question.to_owned(),
                    "--criteria".into(),
                    criteria.to_owned(),
                    "--no-run".into(),
                ],
                &["APB_RUN_ID", "APB_NODE_ID"],
                // The caller's cutoff kills the child too (R3-9).
                self.client.timing.decide.min(limit),
            )
            .await?;
        let line = out.stdout.lines().next().unwrap_or_default().trim();
        let v: Value = serde_json::from_str(line).map_err(|_| RunnerError::Protocol {
            detail: format!(
                "apb decide: exit {:?} with a body that is not one JSON line",
                out.status
            ),
        })?;
        let answered = v.get("answered").and_then(Value::as_bool);
        match (answered, out.status) {
            (Some(true), _) => {
                let yes = v.get("answer").and_then(Value::as_str) == Some("yes");
                let p = v.get("p").and_then(decimal);
                Ok(DecideOutcome::Answered { yes, p })
            }
            (Some(false), Some(1)) => {
                if let Some(code) = v.get("refused").and_then(Value::as_str) {
                    return Ok(DecideOutcome::Unanswered {
                        reason: code.to_owned(),
                    });
                }
                if v.get("error").is_some() {
                    return Ok(DecideOutcome::Unanswered {
                        reason: "error".into(),
                    });
                }
                Err(self.client.protocol("apb decide: no answer, no refusal"))
            }
            _ => Err(self.client.protocol(&format!(
                "apb decide: exit {:?} with answered={answered:?}",
                out.status
            ))),
        }
    }

    async fn find(&self, q: RunQuery) -> Result<RunLookup, RunnerError> {
        let term = format!("{}:{}", q.field.name(), q.field.value());
        let mut query: Vec<(&str, &str)> = vec![("workspace", self.workspace.as_str())];
        if let Some(p) = &q.playbook {
            query.push(("playbook", p.as_str()));
        }
        query.push(("field", &term));
        let v = self.client.get("/api/runs", &query, Kind::Read).await?;
        parse_lookup(&v).map_err(|e| self.client.protocol(&format!("GET /api/runs: {e}")))
    }

    async fn inspect(&self, id: &PlaybookId) -> Result<ExecutionFacts, RunnerError> {
        self.inspect_common(None, id).await
    }

    async fn inspect_source(&self, source: &str) -> Result<ExecutionFacts, RunnerError> {
        self.inspect_common(Some(source), &PlaybookId::new(String::new()))
            .await
    }
}

/// An id used as one URL path segment: no separators or dot segments.
pub(crate) fn path_segment(id: &str) -> Result<&str, RunnerError> {
    let ok = !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok {
        Ok(id)
    } else {
        Err(RunnerError::Invalid {
            detail: format!("`{id}` is not a valid apb id"),
        })
    }
}

/// Params with the reserved names, as the runner sends them (for callers
/// that record what was sent).
pub fn sent_params(req: &StartRequest) -> BTreeMap<String, String> {
    let mut p = req.params.clone();
    p.insert("hire_id".into(), req.hire_id.as_str().to_owned());
    p.insert("execution_id".into(), req.execution_id.as_str().to_owned());
    if !req.direct_models.is_empty() {
        p.insert(
            "direct_models".into(),
            serde_json::to_string(&req.direct_models).expect("a string list serializes"),
        );
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported model mapping: `none` is empty, one model is `One`,
    /// everything else (mixed, partial) is `Mixed` (design 7).
    #[test]
    fn reported_kinds_map_to_none_one_and_mixed() {
        let none = parse_reported(&serde_json::json!({"kind": "none"}));
        assert_eq!(none, Reported::None);
        let one = parse_reported(&serde_json::json!({"kind": "one", "model": "m"}));
        assert_eq!(one, Reported::One("m".into()));
        let one_without_model = parse_reported(&serde_json::json!({"kind": "one"}));
        assert_eq!(one_without_model, Reported::Mixed);
        let partial = parse_reported(
            &serde_json::json!({"kind": "partial", "known": ["m"], "unknown_tasks": 1}),
        );
        assert_eq!(partial, Reported::Mixed);
        let mixed = parse_reported(&serde_json::json!({"kind": "mixed", "models": ["a", "b"]}));
        assert_eq!(mixed, Reported::Mixed);
        let missing = parse_reported(&serde_json::json!({}));
        assert_eq!(missing, Reported::Mixed);
    }
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An HTTP/2 server that refuses every stream with `REFUSED_STREAM`,
    /// the protocol nack reqwest's default policy resends; returns its
    /// address and the number of streams it received.
    async fn refusing_h2_server() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let streams = Arc::new(AtomicUsize::new(0));
        let count = streams.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let count = count.clone();
                tokio::spawn(async move {
                    let Ok(mut conn) = h2::server::handshake(tcp).await else {
                        return;
                    };
                    while let Some(Ok((_req, mut respond))) = conn.accept().await {
                        count.fetch_add(1, Ordering::SeqCst);
                        respond.send_reset(h2::Reason::REFUSED_STREAM);
                    }
                });
            }
        });
        (addr, streams)
    }

    #[tokio::test]
    async fn a_refused_http2_stream_is_not_resent() {
        let (addr, streams) = refusing_h2_server().await;
        // The production transport settings, over cleartext HTTP/2.
        let http = http_client_builder(Duration::from_secs(2))
            .http2_prior_knowledge()
            .build()
            .unwrap();
        let sent = http
            .post(format!("http://{addr}/api/playbooks/p/run"))
            .json(&serde_json::json!({"params": {}}))
            .send()
            .await;
        assert!(sent.is_err(), "{sent:?}");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(streams.load(Ordering::SeqCst), 1, "the request was resent");
    }
}
