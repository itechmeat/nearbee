//! The scripted runner: runs are records in memory whose status the
//! scenario sets; a run marked succeeded leaves the fast audit's real
//! output files in its worktree (S6 design 1.4).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nearbee_core::journal::sha256_hex;
use nearbee_core::manifest::{Entry, EvidenceEntry, Manifest, Visibility};
use nearbee_core::runner::{
    DecideOutcome, Estimate, ExecutionFacts, PlaybookId, PlaybookSummary, RunDetail, RunId,
    RunLookup, RunQuery, RunStatus, RunSummary, Runner, RunnerError, StartRequest, Usage,
    WorkspaceId,
};
use nearbee_core::signature::SignatureFile;
use nearbee_core::tiers::{Known, NodeFact, ProviderRoute};

use super::{HANDLE, MODEL, PLAYBOOK_VERSION};

/// The embedded findings and deliverable of the fast audit.
const CANDIDATES: &str = include_str!("fixtures/candidates.json");
const DELIVERABLE: &str = include_str!("fixtures/deliverable.md");
/// The documentation example of a signature answer (placeholder values).
const SIGNATURE_DOCS: &str = include_str!("fixtures/signature_docs.json");
/// The recorded model attestation report.
pub const REPORT: &str = include_str!("fixtures/attestation_report.json");

/// One scripted run.
#[derive(Debug, Clone)]
struct Run {
    status: &'static str,
    /// Polls left before an ordered abort reads terminal: apb's abort
    /// reaches the driver, which stops after its current step.
    abort_polls: Option<u8>,
    params: BTreeMap<String, String>,
    worktree: PathBuf,
    /// The start request's external-data flag, echoed by the detail.
    external_data: bool,
}

#[derive(Debug, Default)]
struct Inner {
    runs: BTreeMap<String, Run>,
    starts: Vec<StartRequest>,
    /// The next start lands but its answer is lost (the restart scenario).
    lose_next_answer: bool,
    /// What the runner did since the last [`ReplayRunner::take_events`].
    events: Vec<String>,
    finds: usize,
    aborts: usize,
}

/// The replay's [`Runner`].
#[derive(Debug)]
pub struct ReplayRunner {
    workspace: WorkspaceId,
    inner: Mutex<Inner>,
}

impl ReplayRunner {
    pub fn new() -> Self {
        Self {
            workspace: WorkspaceId::new("replay-workspace"),
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The next start lands and its answer is lost.
    pub fn lose_next_answer(&self) {
        self.lock().lose_next_answer = true;
    }

    /// Every start request, in order.
    pub fn starts(&self) -> Vec<StartRequest> {
        self.lock().starts.clone()
    }

    pub fn finds(&self) -> usize {
        self.lock().finds
    }

    pub fn aborts(&self) -> usize {
        self.lock().aborts
    }

    /// The id of the newest run.
    pub fn latest_run(&self) -> Option<String> {
        self.lock()
            .starts
            .last()
            .map(|s| run_id_of(s.execution_id.as_str()))
    }

    /// Marks the newest run succeeded and writes its output files.
    pub fn finish_latest(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let Some(id) = inner
            .starts
            .last()
            .map(|s| run_id_of(s.execution_id.as_str()))
        else {
            return Err("no run to finish".into());
        };
        let run = inner.runs.get_mut(&id).ok_or("the run is unknown")?;
        write_outputs(&run.worktree)?;
        run.status = "succeeded";
        Ok(())
    }

    /// What the runner did since the last call.
    pub fn take_events(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().events)
    }
}

fn run_id_of(execution_id: &str) -> String {
    format!("run-{execution_id}")
}

/// The execution facts of the fast listing's playbook as apb would report
/// them: script nodes only, the shipped `nearai` route, decisions and
/// compaction off. The direct models come from the listing record.
pub fn facts() -> ExecutionFacts {
    let node = |id: &str, kind: &str| NodeFact {
        id: id.into(),
        kind: kind.into(),
        interactive: false,
        profile: None,
        connectors: Vec::new(),
        playbook_ref: None,
    };
    ExecutionFacts {
        playbook_id: HANDLE.into(),
        default_profile: None,
        nodes: vec![
            node("start", "start"),
            node("fetch", "script"),
            node("review", "script"),
            node("finish", "script"),
            node("done", "finish"),
        ],
        profiles: Vec::new(),
        routes: Known::Yes(vec![ProviderRoute {
            name: "nearai".into(),
            base_url: nearbee_core::tiers::NEARAI_BASE_URL.into(),
            api_key_ref: nearbee_core::tiers::NEARAI_API_KEY_REF.into(),
            model_overrides: Vec::new(),
        }]),
        decisions_enabled: Known::Yes(false),
        compaction_enabled: Known::Yes(false),
        direct_models: Vec::new(),
        signed_models: Vec::new(),
    }
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join(name), bytes).map_err(|e| format!("{name}: {e}"))
}

/// The request, response and `signature.json` bytes of the signed call.
struct SignedFiles {
    request: Vec<u8>,
    response: Vec<u8>,
    signature: Vec<u8>,
}

/// The signed call's request and response bytes and the synthetic
/// signature that binds them: the documentation example's shape, with
/// placeholder address and signature (they will not verify).
fn signed_files() -> Result<SignedFiles, String> {
    let request = serde_json::to_vec(&serde_json::json!({
        "model": MODEL,
        "stream": false,
        "max_tokens": crate::audit::REVIEW_MAX_TOKENS,
        "messages": [
            {"role": "system", "content": "Replay fixture: the review rules."},
            {"role": "user", "content": "Replay fixture: the packed repository."}
        ]
    }))
    .map_err(|e| e.to_string())?;
    let response = serde_json::to_vec(&serde_json::json!({
        "id": "chatcmpl-replay-review",
        "object": "chat.completion",
        "model": MODEL,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": CANDIDATES},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 2048, "completion_tokens": 256, "total_tokens": 2304}
    }))
    .map_err(|e| e.to_string())?;
    let docs: serde_json::Value =
        serde_json::from_str(SIGNATURE_DOCS).map_err(|e| e.to_string())?;
    let body = &docs["body"];
    let (req_sha, resp_sha) = (sha256_hex(&request), sha256_hex(&response));
    let field = |k: &str| body[k].as_str().unwrap_or_default().to_owned();
    let sig = SignatureFile {
        version: 1,
        completion_id: "chatcmpl-replay-review".into(),
        model: MODEL.into(),
        signature_kind: field("signature_kind"),
        signing_algo: field("signing_algo"),
        // The documentation's `0x...` placeholders made well formed: zeros
        // of the right length, which no TEE ever signed with.
        signing_address: format!("0x{}", "0".repeat(40)),
        signed_text: format!("{MODEL}:{req_sha}:{resp_sha}"),
        signature: format!("0x{}", "0".repeat(130)),
        request_sha256: req_sha,
        response_sha256: resp_sha,
    };
    let signature = serde_json::to_vec_pretty(&sig).map_err(|e| e.to_string())?;
    Ok(SignedFiles {
        request,
        response,
        signature,
    })
}

/// The recorded attestation report body.
pub fn report_body() -> Vec<u8> {
    serde_json::from_str::<serde_json::Value>(REPORT)
        .ok()
        .and_then(|v| serde_json::to_vec(&v["body"]).ok())
        .unwrap_or_default()
}

/// The fast audit's `out/`: the deliverable, the findings, the four
/// evidence files and the manifest last.
fn write_outputs(wt: &Path) -> Result<(), String> {
    let out = wt.join("out");
    let evidence = out.join("evidence");
    write(&out, "deliverable.md", DELIVERABLE.as_bytes())?;
    write(&out, "findings.json", CANDIDATES.as_bytes())?;
    let SignedFiles {
        request,
        response,
        signature,
    } = signed_files()?;
    let report = report_body();
    let mut entries = Vec::new();
    for ((name, kind, visibility), bytes) in crate::digest::EVIDENCE
        .into_iter()
        .zip([&signature, &request, &response, &report])
    {
        write(&evidence, name, bytes)?;
        entries.push(EvidenceEntry {
            path: format!("evidence/{name}"),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            kind: kind.into(),
            visibility,
        });
    }
    debug_assert!(entries.iter().any(|e| e.visibility == Visibility::Public));
    let manifest = Manifest {
        version: 1,
        markdown: Entry {
            path: "deliverable.md".into(),
            size: DELIVERABLE.len() as u64,
            sha256: sha256_hex(DELIVERABLE.as_bytes()),
        },
        files: Vec::new(),
        evidence: entries,
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    write(&out, "result.json", &bytes)
}

impl Runner for ReplayRunner {
    fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    async fn playbooks(&self) -> Result<Vec<PlaybookSummary>, RunnerError> {
        Ok(vec![PlaybookSummary {
            id: PlaybookId::new(HANDLE),
            name: "Nearbee repo audit (fast)".into(),
            version: Some(PLAYBOOK_VERSION.into()),
        }])
    }

    async fn estimate(&self, _id: &PlaybookId) -> Result<Estimate, RunnerError> {
        Err(RunnerError::Protocol {
            detail: "the replay has no estimates".into(),
        })
    }

    async fn start(&self, req: StartRequest) -> Result<RunId, RunnerError> {
        let mut inner = self.lock();
        let id = run_id_of(req.execution_id.as_str());
        let mut params = BTreeMap::from([
            ("hire_id".to_owned(), req.hire_id.as_str().to_owned()),
            (
                "execution_id".to_owned(),
                req.execution_id.as_str().to_owned(),
            ),
        ]);
        params.extend(req.params.clone());
        inner.runs.insert(
            id.clone(),
            Run {
                status: "running",
                abort_polls: None,
                params,
                worktree: req.worktree.clone(),
                external_data: req.external_data,
            },
        );
        let continued = req.continued_from.is_some();
        inner.starts.push(req);
        if std::mem::take(&mut inner.lose_next_answer) {
            inner
                .events
                .push("apb: run started, its answer lost".into());
            return Err(RunnerError::StartOutcomeUnknown {
                detail: "the start answer was lost (scripted)".into(),
            });
        }
        inner.events.push(if continued {
            "apb: continued run started with the feedback".into()
        } else {
            "apb: run started".into()
        });
        Ok(RunId::new(id))
    }

    async fn external_data_supported(&self) -> Result<bool, RunnerError> {
        Ok(true)
    }

    async fn run(&self, id: &RunId) -> Result<RunDetail, RunnerError> {
        let mut inner = self.lock();
        let run = inner
            .runs
            .get_mut(id.as_str())
            .ok_or_else(|| RunnerError::Protocol {
                detail: "unknown run".into(),
            })?;
        match run.abort_polls {
            Some(0) => {
                run.status = "aborted";
                run.abort_polls = None;
            }
            Some(n) => run.abort_polls = Some(n - 1),
            None => {}
        }
        let run = run.clone();
        let status = RunStatus::parse(run.status);
        let usage = if status.is_terminal() {
            Usage {
                cost_usd: Some(rust_decimal::Decimal::new(3, 3)),
                attempts: Some(3),
                finished_attempts: Some(3),
                cost_attempts: Some(3),
                ..Usage::default()
            }
        } else {
            Usage::default()
        };
        Ok(RunDetail {
            id: id.clone(),
            playbook: PlaybookId::new(HANDLE),
            playbook_version: Some(PLAYBOOK_VERSION.into()),
            status,
            nodes: Vec::new(),
            worktree: Some(run.worktree.clone()),
            params: run.params.clone(),
            usage,
            budget_paused: false,
            attempts: Vec::new(),
            external_data: run.external_data,
        })
    }

    async fn stop(&self, _id: &RunId) -> Result<(), RunnerError> {
        Ok(())
    }

    async fn abort(&self, id: &RunId) -> Result<(), RunnerError> {
        let mut inner = self.lock();
        inner.aborts += 1;
        if let Some(run) = inner.runs.get_mut(id.as_str())
            && !RunStatus::parse(run.status).is_terminal()
            && run.abort_polls.is_none()
        {
            run.abort_polls = Some(1);
            inner.events.push("apb: abort sent to the run".into());
        }
        Ok(())
    }

    async fn decide(
        &self,
        _question: &str,
        _criteria: &str,
        _limit: std::time::Duration,
    ) -> Result<DecideOutcome, RunnerError> {
        self.lock()
            .events
            .push("apb: scope check answered yes".into());
        Ok(DecideOutcome::Answered {
            yes: true,
            p: Some(rust_decimal::Decimal::new(9, 1)),
        })
    }

    async fn find(&self, q: RunQuery) -> Result<RunLookup, RunnerError> {
        let mut inner = self.lock();
        inner.finds += 1;
        let runs: Vec<RunSummary> = inner
            .runs
            .iter()
            .filter(|(_, r)| {
                r.params.get(q.field.name()).map(String::as_str) == Some(q.field.value())
            })
            .map(|(id, r)| RunSummary {
                id: RunId::new(id.clone()),
                playbook: PlaybookId::new(HANDLE),
                status: RunStatus::parse(r.status),
            })
            .collect();
        let found = runs.len();
        inner.events.push(format!(
            "apb: lookup by {} found {found} run{}",
            q.field.name().replace('_', " "),
            if found == 1 { "" } else { "s" }
        ));
        Ok(RunLookup {
            runs,
            complete: true,
            withheld: 0,
            not_indexed: 0,
        })
    }

    async fn inspect(&self, _id: &PlaybookId) -> Result<ExecutionFacts, RunnerError> {
        Ok(facts())
    }

    async fn inspect_source(&self, _source: &str) -> Result<ExecutionFacts, RunnerError> {
        Ok(facts())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nearbee_core::runner::{ExecutionId, HireId};

    fn request(execution: &str, external_data: bool) -> StartRequest {
        StartRequest {
            playbook: PlaybookId::new(HANDLE),
            instruction: String::new(),
            params: BTreeMap::new(),
            worktree: PathBuf::from("/nonexistent"),
            hire_id: HireId::new("h1"),
            execution_id: ExecutionId::new(execution),
            budget: None,
            continued_from: None,
            direct_models: Vec::new(),
            external_data,
        }
    }

    /// The scripted run detail echoes the start request's flag.
    #[tokio::test]
    async fn the_run_detail_echoes_the_external_data_flag() {
        let r = ReplayRunner::new();
        let on = r.start(request("h1-1", true)).await.unwrap();
        let off = r.start(request("h1-2", false)).await.unwrap();
        assert!(r.run(&on).await.unwrap().external_data);
        assert!(!r.run(&off).await.unwrap().external_data);
    }
}
