//! The `nearbee` command line: `config check` and `runs find`.
//!
//! The commands write to the given writer and return the process exit code,
//! so tests drive them without spawning the binary.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use nearbee_apb::{ApbClient, ApbOptions, ApbRunner, ConnectError, Timing, resolve_workspace};
use nearbee_core::config::{self, Config, LoadOptions, VARIABLES};
use nearbee_core::credentials::{self, CredentialsError, Store};
use nearbee_core::market::{Market as _, MarketError};
use nearbee_core::redact::redact_detail;
use nearbee_core::runner::{ExecutionId, HireId, PlaybookId, RunQuery, Runner, SearchField};
use nearbee_core::secret::Secret;
use nearbee_market::NearAgentMarket;

pub mod audit;
pub mod digest;
pub mod publish;
pub mod replay;
pub mod signed_call;

/// Exit code for an error that is not a finding (unreadable config,
/// unreachable apb).
pub const EXIT_ERROR: i32 = 3;

#[derive(Debug, Parser)]
#[command(
    name = "nearbee",
    version,
    about = "Turns apb playbooks into agents for hire"
)]
pub struct Cli {
    /// Configuration file (default: nearbee.yaml in the working directory).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Configuration commands.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run commands.
    Runs {
        #[command(subcommand)]
        action: RunsAction,
    },
    /// Agent commands.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Publish a listing: materialise the playbook, patch the agent, set
    /// the pricing plan.
    List(ListArgs),
    /// Serve hires for the published listings.
    Serve(ServeArgs),
    /// List the journaled hires (read only, no lock).
    Hires(HiresArgs),
    /// Delete the files of hires past their retention (takes the serve
    /// lock).
    Cleanup(CleanupArgs),
    /// Agent A's steps, run by the `repo-audit` playbook's script nodes in
    /// the run's worktree.
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },
    /// Send one exact-bytes NEAR AI Cloud completion and keep its response
    /// and its `provider_tee` signature (run by a script node).
    SignedCall(SignedCallArgs),
    /// Agent B's steps, run by the `doc-digest` playbook's script nodes in
    /// the run's worktree.
    Digest {
        #[command(subcommand)]
        action: DigestAction,
    },
    /// Walk five scripted hires through the real serve state machine,
    /// with no account, no apb and no network.
    Replay(ReplayArgs),
}

#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// Run only this scenario (repeatable): success, decline, redo,
    /// restart, cancellation.
    #[arg(long = "scenario", value_name = "NAME")]
    pub scenarios: Vec<String>,
    /// Keep the state in this new or empty directory.
    #[arg(long, value_name = "DIR")]
    pub keep: Option<PathBuf>,
    /// Name the journal events each step wrote.
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum DigestAction {
    /// Render the attached PDF and image files into page images.
    Pages,
    /// Transcribe every page on the vision model.
    Extract,
    /// Count personal data per page with the privacy classifier.
    Classify,
    /// Make the signed digest call and write the deliverable.
    Compose,
}

#[derive(Debug, Args)]
pub struct SignedCallArgs {
    /// The request file, sent unchanged.
    #[arg(long)]
    pub request: PathBuf,
    /// The state and evidence directory.
    #[arg(long)]
    pub out: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum AuditAction {
    /// Take a snapshot of the one public GitHub repository the run's
    /// brief names into `audit-snapshot/`.
    Fetch,
    /// The fast variant's review: pack the snapshot and write
    /// `audit/candidates.json` from one signed NEAR AI Cloud call.
    Review,
    /// Check `audit/candidates.json` against the snapshot and write
    /// `out/findings.json`, `out/deliverable.md` and `out/result.json`.
    Finish,
}

#[derive(Debug, Args)]
pub struct HiresArgs {
    /// Print JSON instead of lines.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CleanupArgs {
    /// Print what would be deleted and change nothing (no lock).
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Load the configuration, report variables by name, reach apb and
    /// resolve the workspace.
    Check,
}

#[derive(Debug, Subcommand)]
pub enum AgentAction {
    /// Store the token of one of the owner's agents (the token is read from
    /// standard input, never from arguments or the environment).
    Add {
        /// The agent's handle.
        handle: String,
        /// The agent id, for a draft agent whose handle the public lookup
        /// cannot see; copy it from the agent's settings page in the market
        /// web app.
        #[arg(long)]
        agent_id: Option<String>,
    },
    /// Print the handles stored in the credentials store, with masked
    /// tokens.
    List,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// The listing file.
    pub listing: PathBuf,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Decline every new first-time hire of this handle with `rejected`
    /// before any work (the x402 proof).
    #[arg(long)]
    pub reject: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RunsAction {
    /// Find the runs of a hire or an execution. Exit 0: found; 1: none and
    /// the search was complete; 2: none but some runs could not be searched.
    Find(FindArgs),
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct FindIds {
    /// Market hire id.
    #[arg(long, value_parser = non_blank)]
    pub hire_id: Option<String>,
    /// Nearbee execution id.
    #[arg(long, value_parser = non_blank)]
    pub execution_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct FindArgs {
    #[command(flatten)]
    pub ids: FindIds,
    /// Limit the search to one playbook.
    #[arg(long, value_parser = non_blank)]
    pub playbook: Option<String>,
}

/// Refuses an empty or whitespace-only value: it would match nothing or
/// everything instead of the run the caller meant.
fn non_blank(v: &str) -> Result<String, String> {
    if v.trim().is_empty() {
        Err("must not be empty or blank".into())
    } else {
        Ok(v.to_owned())
    }
}

/// The process environment and working directory a command sees.
pub struct Env<'a> {
    pub cwd: &'a Path,
    pub var: &'a dyn Fn(&str) -> Option<String>,
    pub timing: Timing,
}

fn load(cli_config: Option<&Path>, env: &Env<'_>) -> Result<Config, config::ConfigError> {
    config::load(
        LoadOptions {
            file: cli_config,
            cwd: env.cwd,
        },
        |k| (env.var)(k),
    )
}

fn apb_options(cfg: &Config, env: &Env<'_>) -> ApbOptions {
    ApbOptions {
        url: cfg.apb.url.clone(),
        api_key: cfg.apb.api_key.clone(),
        redact: cfg.secrets(),
        timing: env.timing.clone(),
        bin: apb_bin(env),
        config_dir: cfg.apb.config_dir.clone(),
        systemctl_bin: systemctl_bin(env),
        xdg_config_home: cfg.apb.xdg_config_home.clone(),
    }
}

/// Writes one line with every configured secret removed, whatever the line
/// interpolates (a URL, an error, an apb answer).
pub struct Lines<'a> {
    pub out: &'a mut dyn Write,
    pub secrets: Vec<Secret>,
}

impl<'a> Lines<'a> {
    pub fn new(out: &'a mut dyn Write) -> Self {
        Self {
            out,
            secrets: Vec::new(),
        }
    }

    pub fn line(&mut self, text: &str) {
        let _ = writeln!(self.out, "{}", redact_detail(text, &self.secrets));
    }
}

/// Runs a parsed command line; returns the exit code.
pub async fn run(cli: Cli, env: &Env<'_>, out: &mut dyn Write) -> i32 {
    match cli.command {
        Command::Config {
            action: ConfigAction::Check,
        } => config_check(cli.config.as_deref(), env, out).await,
        Command::Runs {
            action: RunsAction::Find(args),
        } => runs_find(cli.config.as_deref(), args, env, out).await,
        Command::Agent {
            action: AgentAction::Add { handle, agent_id },
        } => agent_add(cli.config.as_deref(), &handle, agent_id, env, out).await,
        Command::Agent {
            action: AgentAction::List,
        } => agent_list(env, out),
        Command::List(args) => publish_list(cli.config.as_deref(), args, env, out).await,
        Command::Serve(args) => serve(cli.config.as_deref(), args, env, out).await,
        Command::Hires(args) => hires(cli.config.as_deref(), args, env, out),
        Command::Cleanup(args) => cleanup(cli.config.as_deref(), args, env, out),
        Command::Audit {
            action: AuditAction::Fetch,
        } => audit_fetch(env, out).await,
        Command::Audit {
            action: AuditAction::Review,
        } => audit_review(env, out).await,
        Command::Audit {
            action: AuditAction::Finish,
        } => audit_finish(env, out),
        Command::SignedCall(args) => signed_call_command(env, &args.request, &args.out, out).await,
        Command::Digest { action } => digest_command(env, action, out).await,
        Command::Replay(args) => {
            let opts = replay::Options {
                scenarios: args.scenarios,
                keep: args.keep.map(|k| env.cwd.join(k)),
                verbose: args.verbose,
            };
            replay::run(&opts, out).await
        }
    }
}

/// `nearbee digest <step>` in the current directory, against
/// [`nearbee_cloud::PRODUCTION_BASE`].
pub async fn digest_command(env: &Env<'_>, action: DigestAction, out: &mut dyn Write) -> i32 {
    let mut lines = Lines::new(out);
    let step = format!("{action:?}").to_lowercase();
    let result = digest_step(env, action, &mut lines).await;
    match result {
        Ok(text) => {
            lines.line(&format!("digest {step}: {text}"));
            0
        }
        Err(e) => {
            lines.line(&format!("error: digest {step}: {e}"));
            EXIT_ERROR
        }
    }
}

async fn digest_step(
    env: &Env<'_>,
    action: DigestAction,
    lines: &mut Lines<'_>,
) -> Result<String, String> {
    let params = script_params(env)?;
    if let DigestAction::Pages = action {
        let tools = digest::Tools::resolve((env.var)("PATH").as_deref())?;
        let p = digest::pages(env.cwd, &params, &tools)?;
        return Ok(format!(
            "{} pages rendered, {} skipped, {} files unusable",
            p.pages.len(),
            p.skipped,
            p.unusable
        ));
    }
    let key = cloud_key(env)?;
    lines.secrets.push(key.clone());
    let client = nearbee_cloud::CloudClient::new(nearbee_cloud::PRODUCTION_BASE, key.clone())?;
    match action {
        DigestAction::Pages => unreachable!("handled above"),
        DigestAction::Extract => {
            let n = digest::extract(env.cwd, &params, &client).await?;
            Ok(format!("{n} pages extracted"))
        }
        DigestAction::Classify => {
            let p = digest::classify(env.cwd, &params, &client).await?;
            Ok(format!("{} pages classified", p.pages.len()))
        }
        DigestAction::Compose => {
            let attestor = nearbee_cloud::CloudAttestor::new(nearbee_cloud::PRODUCTION_BASE, key)?
                .with_provider("near");
            let c = digest::compose(
                env.cwd,
                &params,
                &client,
                &attestor,
                &signed_call::Timing::default(),
            )
            .await?;
            Ok(format!(
                "deliverable written: {} pages digested, {} figures kept, {} dropped",
                c.digest.pages.digested,
                c.digest.figures.len(),
                c.digest.dropped_figures
            ))
        }
    }
}

/// The NEAR AI Cloud key of Nearbee's commands (`NEARAI_API_KEY`).
fn cloud_key(env: &Env<'_>) -> Result<Secret, String> {
    match (env.var)("NEARAI_API_KEY") {
        Some(k) if !k.trim().is_empty() => Ok(Secret::new(k.trim())),
        _ => Err(nearbee_cloud::NO_KEY.to_owned()),
    }
}

/// The run params of a script node (`APB_RUN_DIR`).
fn script_params(env: &Env<'_>) -> Result<std::collections::BTreeMap<String, String>, String> {
    let run_dir = (env.var)("APB_RUN_DIR").map(PathBuf::from);
    nearbee_apb::run_params::from_run_dir(run_dir.as_deref())
}

/// `nearbee signed-call --request <file> --out <dir>` against
/// [`nearbee_cloud::PRODUCTION_BASE`].
pub async fn signed_call_command(
    env: &Env<'_>,
    request: &Path,
    out_dir: &Path,
    out: &mut dyn Write,
) -> i32 {
    let mut lines = Lines::new(out);
    let key = match cloud_key(env) {
        Ok(k) => k,
        Err(e) => {
            lines.line(&format!("error: signed-call: {e}"));
            return EXIT_ERROR;
        }
    };
    lines.secrets.push(key.clone());
    let allowed = match script_params(env).and_then(|p| signed_call::parse_direct_models(&p)) {
        Ok(m) => m,
        Err(e) => {
            lines.line(&format!("error: signed-call: {e}"));
            return EXIT_ERROR;
        }
    };
    let built = nearbee_cloud::CloudClient::new(nearbee_cloud::PRODUCTION_BASE, key.clone())
        .and_then(|c| {
            nearbee_cloud::CloudAttestor::new(nearbee_cloud::PRODUCTION_BASE, key)
                .map(|a| (c, a.with_provider("near")))
        });
    let (client, attestor) = match built {
        Ok(p) => p,
        Err(e) => {
            lines.line(&format!("error: signed-call: {e}"));
            return EXIT_ERROR;
        }
    };
    let request = env.cwd.join(request);
    let out_dir = env.cwd.join(out_dir);
    match signed_call::signed_call(
        &client,
        &attestor,
        &request,
        &out_dir,
        &allowed,
        &signed_call::Timing::default(),
    )
    .await
    {
        Ok(s) => {
            lines.line(&format!(
                "signed call done: completion {} on {}",
                s.completion_id, s.model
            ));
            0
        }
        Err(e) => {
            lines.line(&format!("error: signed-call: {e}"));
            EXIT_ERROR
        }
    }
}

/// `nearbee audit fetch`: the brief comes from the run's params
/// (`APB_RUN_DIR`), the snapshot goes into the current directory.
pub async fn audit_fetch(env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut lines = Lines::new(out);
    let run_dir = (env.var)("APB_RUN_DIR").map(PathBuf::from);
    let params = match nearbee_apb::run_params::from_run_dir(run_dir.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            lines.line(&format!("error: {e}"));
            return EXIT_ERROR;
        }
    };
    let brief = params.get("brief").cloned().unwrap_or_default();
    match audit::fetch_snapshot(audit::API_BASE, audit::CODELOAD_BASE, &brief, env.cwd).await {
        Ok(audit::Fetched::Published { sha, coverage }) => {
            lines.line(&format!(
                "snapshot published: commit {sha}, {} files, {} members skipped",
                coverage.files_written,
                coverage.skipped_total()
            ));
            0
        }
        Ok(audit::Fetched::Reused { sha }) => {
            lines.line(&format!("snapshot reused: commit {sha}"));
            0
        }
        Err(e) => {
            lines.line(&format!("error: audit fetch: {e}"));
            EXIT_ERROR
        }
    }
}

/// `nearbee audit review` in the current directory, against
/// [`nearbee_cloud::PRODUCTION_BASE`]: the run's params come from
/// `APB_RUN_DIR`, the key from `NEARAI_API_KEY`.
pub async fn audit_review(env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut lines = Lines::new(out);
    let result = async {
        let params = script_params(env)?;
        let key = cloud_key(env)?;
        lines.secrets.push(key.clone());
        let client = nearbee_cloud::CloudClient::new(nearbee_cloud::PRODUCTION_BASE, key.clone())?;
        let attestor = nearbee_cloud::CloudAttestor::new(nearbee_cloud::PRODUCTION_BASE, key)?
            .with_provider("near");
        audit::review(
            env.cwd,
            &params,
            &client,
            &attestor,
            &signed_call::Timing::default(),
        )
        .await
    }
    .await;
    match result {
        Ok(r) => {
            lines.line(&format!(
                "review done: {} candidate findings, {} files packed, {} omitted",
                r.findings, r.packed, r.omitted
            ));
            0
        }
        Err(e) => {
            lines.line(&format!("error: audit review: {e}"));
            EXIT_ERROR
        }
    }
}

/// `nearbee audit finish` in the current directory; the execution id
/// comes from the run's params when `APB_RUN_DIR` is set.
pub fn audit_finish(env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut lines = Lines::new(out);
    // Signed evidence is collected only for a run with direct models
    // (the fast playbook); a premium run has none.
    let (execution_id, direct_models) = match (env.var)("APB_RUN_DIR") {
        None => (None, Vec::new()),
        Some(_) => match script_params(env) {
            Ok(p) => (
                p.get("execution_id").cloned(),
                p.get("direct_models")
                    .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
                    .unwrap_or_default(),
            ),
            Err(e) => {
                lines.line(&format!("error: audit finish: {e}"));
                return EXIT_ERROR;
            }
        },
    };
    match audit::finish(env.cwd, execution_id.as_deref(), &direct_models) {
        Ok(f) => {
            lines.line(&format!(
                "deliverable written: {} findings location checked, {} rejected",
                f.accepted.len(),
                f.rejected.len()
            ));
            0
        }
        Err(e) => {
            lines.line(&format!("error: audit finish: {e}"));
            EXIT_ERROR
        }
    }
}

/// `nearbee config check`: one line per check, `ok`, `missing` or `failed`.
pub async fn config_check(file: Option<&Path>, env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut failed = false;
    let mut lines = Lines::new(out);
    let mut line = |status: &str, text: String| {
        lines.line(&format!("{status:<8}{text}"));
    };
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            line("failed", format!("config: {e}"));
            return 1;
        }
    };
    drop(line);
    lines.secrets = cfg.secrets();
    let mut line = |status: &str, text: String| {
        lines.line(&format!("{status:<8}{text}"));
    };
    match &cfg.file {
        Some(p) => line("ok", format!("config file {}", p.display())),
        None => line("ok", "config: built-in defaults (no nearbee.yaml)".into()),
    }
    for v in VARIABLES {
        let kind = if v.required { "required" } else { "optional" };
        if cfg.present.get(v.name).copied().unwrap_or(false) {
            line("ok", format!("{} is set ({kind})", v.name));
        } else {
            if v.required {
                failed = true;
            }
            line("missing", format!("{} ({kind}): {}", v.name, v.purpose));
        }
    }
    let client = match ApbClient::new(apb_options(&cfg, env)) {
        Ok(c) => c,
        Err(e) => {
            line("failed", format!("apb {}: {e}", cfg.apb.url));
            return 1;
        }
    };
    let projects = match client.projects().await {
        Ok(p) => {
            line(
                "ok",
                format!(
                    "apb dashboard {} answers ({} projects)",
                    cfg.apb.url,
                    p.len()
                ),
            );
            p
        }
        Err(e) => {
            line("failed", format!("apb dashboard {}: {e}", cfg.apb.url));
            return 1;
        }
    };
    match resolve_workspace(&projects, &cfg.apb.workspace_path) {
        Ok(ws) => line(
            "ok",
            format!("apb workspace {} is {ws}", cfg.apb.workspace_path.display()),
        ),
        Err(e) => {
            failed = true;
            line("failed", format!("apb workspace: {e}"));
        }
    }
    // Every market run is an external-data run (S6 design 3.7).
    let min = nearbee_apb::EXTERNAL_DATA_MIN;
    match client.instance_version().await {
        Ok((v, true)) => line(
            "ok",
            format!("apb version {v} (external data runs supported)"),
        ),
        Ok((v, false)) => {
            failed = true;
            line(
                "failed",
                format!("apb version {v}: Nearbee needs {min} or newer"),
            );
        }
        Err(e) => {
            failed = true;
            line(
                "failed",
                format!("apb version: {e}; Nearbee needs {min} or newer"),
            );
        }
    }
    i32::from(failed)
}

/// `nearbee runs find`.
pub async fn runs_find(
    file: Option<&Path>,
    args: FindArgs,
    env: &Env<'_>,
    out: &mut dyn Write,
) -> i32 {
    let mut out = Lines::new(out);
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            out.line(&format!("error: config: {e}"));
            return EXIT_ERROR;
        }
    };
    out.secrets = cfg.secrets();
    let runner = match ApbRunner::connect(apb_options(&cfg, env), &cfg.apb.workspace_path).await {
        Ok(r) => r,
        Err(ConnectError::Runner(e)) => {
            out.line(&format!("error: apb {}: {e}", cfg.apb.url));
            return EXIT_ERROR;
        }
        Err(ConnectError::Workspace(e)) => {
            out.line(&format!("error: {e}"));
            return EXIT_ERROR;
        }
    };
    let field = match (args.ids.hire_id, args.ids.execution_id) {
        (Some(h), None) => SearchField::HireId(HireId::new(h)),
        (None, Some(x)) => SearchField::ExecutionId(ExecutionId::new(x)),
        _ => {
            out.line("error: give exactly one of --hire-id or --execution-id");
            return EXIT_ERROR;
        }
    };
    let query = RunQuery {
        playbook: args.playbook.map(PlaybookId::new),
        field,
    };
    let lookup = match runner.find(query).await {
        Ok(l) => l,
        Err(e) => {
            out.line(&format!("error: {e}"));
            return EXIT_ERROR;
        }
    };
    for r in &lookup.runs {
        out.line(&format!("{}\t{}\t{}", r.id, r.playbook, r.status));
    }
    if lookup.complete {
        out.line("complete: yes");
    } else {
        out.line(&format!(
            "complete: no ({} runs not indexed, {} withheld)",
            lookup.not_indexed, lookup.withheld
        ));
    }
    match (lookup.runs.is_empty(), lookup.complete) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    }
}

/// The credentials store for this process, from `XDG_CONFIG_HOME` and
/// `HOME`. `NEARBEE_CREDENTIALS` is a real operator override (an
/// alternative store path, documented in README.md), not a test hook.
pub fn store(env: &Env<'_>) -> Option<Store> {
    if let Some(p) = (env.var)("NEARBEE_CREDENTIALS").filter(|v| !v.is_empty()) {
        return Some(Store::at(PathBuf::from(p)));
    }
    credentials::credentials_path(
        (env.var)("XDG_CONFIG_HOME").as_deref(),
        (env.var)("HOME").as_deref(),
    )
    .map(Store::at)
}

fn market_options(
    base: String,
    token: Secret,
    secrets: Vec<Secret>,
) -> nearbee_market::MarketOptions {
    nearbee_market::MarketOptions {
        base_url: base,
        token,
        redact: secrets,
        timing: Default::default(),
    }
}

/// The market base URL: `MARKET_URL`, default `https://market.near.ai`.
/// The NEAR AI Cloud API base of the attestation reports (design 6.1):
/// `NEARAI_CLOUD_URL` when set, else the production base.
fn nearai_cloud_url(env: &Env<'_>) -> String {
    (env.var)("NEARAI_CLOUD_URL")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| nearbee_cloud::PRODUCTION_BASE.into())
}

/// The attestor of a serve round (design 6.1): the NEAR AI Cloud client
/// when an attested listing is served and the key is configured; else an
/// attestor whose every fetch fails with its reason and sends nothing.
enum ServeAttestor {
    Cloud(Box<nearbee_cloud::CloudAttestor>),
    Off(nearbee_core::evidence::Unavailable),
}

impl nearbee_core::evidence::Attestor for ServeAttestor {
    async fn report(&self, model: &str, nonce: &str) -> Result<Vec<u8>, String> {
        match self {
            ServeAttestor::Cloud(c) => c.report(model, nonce).await,
            ServeAttestor::Off(u) => u.report(model, nonce).await,
        }
    }
}

fn serve_attestor<'a>(
    cfg: &Config,
    env: &Env<'_>,
    mut contexts: impl Iterator<Item = &'a nearbee_core::serve::ListingContext>,
) -> ServeAttestor {
    use nearbee_core::evidence::Unavailable;
    let attested = contexts.any(|c| {
        c.record
            .as_ref()
            .is_some_and(|r| r.tier == Some(nearbee_core::listing::Tier::Attested))
    });
    if !attested {
        return ServeAttestor::Off(Unavailable("no attested listing is served".into()));
    }
    let Some(key) = cfg.nearai_api_key.clone() else {
        return ServeAttestor::Off(Unavailable(nearbee_cloud::NO_KEY.into()));
    };
    match nearbee_cloud::CloudAttestor::new(nearai_cloud_url(env), key) {
        Ok(c) => ServeAttestor::Cloud(Box::new(c)),
        Err(e) => ServeAttestor::Off(Unavailable(e)),
    }
}

fn market_url(env: &Env<'_>) -> String {
    (env.var)("MARKET_URL")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://market.near.ai".into())
}

/// `nearbee agent add <handle> [--agent-id <uuid>]`: read the token from
/// stdin, resolve the agent id from the public record, prove the token with
/// an authenticated read, store it. A draft agent is invisible to every
/// lookup its token can make (measured 2026-11-10): when the public lookup
/// answers 404 the id has to come from the operator (`--agent-id`, copied
/// from the agent's settings page in the market web app).
pub async fn agent_add(
    file: Option<&Path>,
    handle: &str,
    agent_id: Option<String>,
    env: &Env<'_>,
    out: &mut dyn Write,
) -> i32 {
    let mut stdin = std::io::stdin();
    agent_add_from(file, handle, agent_id, env, out, &mut stdin).await
}

/// [`agent_add`] with the token source given, so tests do not touch the
/// process's standard input.
pub async fn agent_add_from(
    file: Option<&Path>,
    handle: &str,
    agent_id: Option<String>,
    env: &Env<'_>,
    out: &mut dyn Write,
    input: &mut dyn std::io::Read,
) -> i32 {
    let mut out = Lines::new(out);
    let Some(store) = store(env) else {
        out.line("error: cannot find a credentials directory (HOME is unset)");
        return EXIT_ERROR;
    };
    // The token is read to EOF and trimmed; the shape is checked before any
    // network call, and the value is never printed.
    let mut raw = Vec::new();
    if std::io::Read::read_to_end(input, &mut raw).is_err() {
        out.line("error: cannot read the token from standard input");
        return EXIT_ERROR;
    }
    let token = String::from_utf8_lossy(&raw).trim().to_owned();
    if !credentials::token_shape_ok(&token) {
        out.line("error: the value on standard input does not look like an agent token (aat_ plus 32 hex digits)");
        return 1;
    }
    let token = Secret::new(token);
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            out.line(&format!("error: config: {e}"));
            return EXIT_ERROR;
        }
    };
    out.secrets = cfg.secrets();
    let mut secrets = cfg.secrets();
    secrets.push(token.clone());
    let client = match NearAgentMarket::new(market_options(market_url(env), token.clone(), secrets))
    {
        Ok(c) => c,
        Err(e) => {
            out.line(&format!("error: market: {e}"));
            return EXIT_ERROR;
        }
    };
    // The operator-given agent id must be a UUID; it is canonicalised so
    // the comparison with the record and the stored value are exact.
    let given_id = match agent_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => match uuid::Uuid::parse_str(raw) {
            Ok(id) => Some(id.to_string()),
            Err(_) => {
                out.line("error: --agent-id must be the agent id (a UUID) from the agent's settings page in the market web app");
                return 1;
            }
        },
    };
    let (agent_id, id_source) = match client.agent(handle).await {
        Ok(record) => {
            if let Some(given) = given_id.as_deref()
                && given != record.agent_id
            {
                out.line(&format!(
                    "error: --agent-id {} differs from the agent id {} the market returns for {handle}; give neither or the right one",
                    given, record.agent_id
                ));
                return 1;
            }
            (record.agent_id, "the market record")
        }
        // The lookup cannot see this agent: a draft is invisible to every
        // lookup its own token can make, so the handle proves nothing about
        // the id. Without an operator-given id the command cannot go on.
        Err(MarketError::Refused { status: 404, .. }) => match given_id {
            Some(id) => (
                id,
                "the operator (--agent-id; the public lookup answers 404 for this draft agent)",
            ),
            None => {
                out.line(&format!(
                    "error: the handle {handle} is not publicly visible (a draft agent answers 404 to every lookup its token can make); copy the agent id from the agent's settings page in the market web app and pass it as --agent-id"
                ));
                return 1;
            }
        },
        Err(e) => {
            out.line(&format!("error: agent {handle}: {e}"));
            return 1;
        }
    };
    // The token is stored only after an authenticated read succeeds: any
    // failure (401 refused, 403, transport, server error) leaves it unstored.
    if let Err(e) = client.assignments().await {
        out.line(&format!(
            "error: the token was refused (the assignments read failed: {e})"
        ));
        return 1;
    }
    let masked_token = nearbee_core::credentials::masked(&token);
    if let Err(e) = store.put(
        handle,
        nearbee_core::credentials::AgentCredentials {
            agent_id: agent_id.clone(),
            token,
        },
    ) {
        out.line(&format!("error: {}: {e}", store.path().display()));
        return EXIT_ERROR;
    }
    out.line(&format!(
        "added {} (agent id {}, from {}) token {}",
        handle, agent_id, id_source, masked_token
    ));
    0
}

/// The apb binary: `APB_BIN`, default `apb` on the path.
/// The `systemctl` binary the inspection reads the apb unit's
/// environment with: `SYSTEMCTL_BIN`, else `systemctl` on `PATH` (like
/// `APB_BIN` for apb).
fn systemctl_bin(env: &Env<'_>) -> PathBuf {
    (env.var)("SYSTEMCTL_BIN")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("systemctl"))
}

fn apb_bin(env: &Env<'_>) -> PathBuf {
    (env.var)("APB_BIN")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("apb"))
}

/// `nearbee list <listing.yaml>`: materialise the playbook, patch
/// the agent, set the pricing plan; see [`publish::publish`].
pub async fn publish_list(
    file: Option<&Path>,
    args: ListArgs,
    env: &Env<'_>,
    out: &mut dyn Write,
) -> i32 {
    let mut lines = Lines::new(out);
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            lines.line(&format!("error: config: {e}"));
            return EXIT_ERROR;
        }
    };
    lines.secrets = cfg.secrets();
    let Some(store) = store(env) else {
        lines.line("error: cannot find a credentials directory (HOME is unset)");
        return EXIT_ERROR;
    };
    // The config's `apb.config_dir`, as serve uses it: the loader has
    // already refused the operator's everyday instance.
    let pub_env = publish::PublishEnv {
        market_url: market_url(env),
        apb_url: cfg.apb.url.clone(),
        apb_api_key: cfg.apb.api_key.clone(),
        timing: env.timing.clone(),
        apb_bin: apb_bin(env),
        apb_config_dir: cfg.apb.config_dir.clone(),
        apb_xdg_config_home: cfg.apb.xdg_config_home.clone(),
        systemctl_bin: systemctl_bin(env),
        apb_workspace: cfg.apb.workspace_path.clone(),
    };
    // The publish writes plain lines; they go through a buffer so every
    // line is redacted like the rest of the CLI.
    let mut buf = Vec::new();
    let code = publish::publish_list(
        &cfg,
        &args.listing,
        &pub_env,
        &store,
        cfg.secrets(),
        &mut buf,
    )
    .await;
    for line in String::from_utf8_lossy(&buf).lines() {
        lines.line(line);
    }
    code
}

/// `nearbee serve`: polls every published listing that also has
/// credentials and takes its hires, one at a time.
pub async fn serve(
    file: Option<&Path>,
    args: ServeArgs,
    env: &Env<'_>,
    out: &mut dyn Write,
) -> i32 {
    let mut out = Lines::new(out);
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            out.line(&format!("error: config: {e}"));
            return EXIT_ERROR;
        }
    };
    out.secrets = cfg.secrets();
    let Some(store) = store(env) else {
        out.line("error: cannot find a credentials directory (HOME is unset)");
        return EXIT_ERROR;
    };
    // One serve (or cleanup) per state directory: the lock is held for
    // the life of this process (design 12).
    let _lock = match nearbee_core::lock::acquire(&cfg.state_dir) {
        Ok(l) => l,
        Err(e @ nearbee_core::lock::LockError::Held { .. }) => {
            out.line(&format!("error: {e}"));
            return nearbee_core::lock::EXIT_LOCKED;
        }
        Err(e) => {
            out.line(&format!("error: {e}"));
            return EXIT_ERROR;
        }
    };
    let poll = (env.var)("NEARBEE_POLL_SECS")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(nearbee_core::serve::POLL_INTERVAL_MIN_SECS);
    let opts = nearbee_core::serve::ServeOptions {
        reject_handle: args.reject.clone(),
        // The poll interval is validated to the spec: every 30 to 60 s.
        poll_interval_secs: nearbee_core::serve::clamp_poll_interval_secs(poll),
        forbidden: forbidden_secrets(&cfg, &store),
        statement_retention_days: cfg.retention.after_acceptance_days,
        ..Default::default()
    };
    serve_loop(&cfg, &store, &opts, env, &mut out, None).await
}

/// Every secret value a deliverable must never contain: the configured
/// secrets and the stored agent tokens.
fn forbidden_secrets(cfg: &Config, store: &nearbee_core::credentials::Store) -> Vec<String> {
    let mut out: Vec<String> = cfg
        .secrets()
        .iter()
        .map(|s| s.expose().to_owned())
        .collect();
    if let Ok(all) = store.read() {
        for creds in all.values() {
            out.push(creds.token.expose().to_owned());
        }
    }
    out.retain(|s| s.len() >= 8);
    out
}

/// The serve loop over the real clients; one round per poll interval.
/// `rounds` bounds the number of rounds (`None` serves forever).
pub async fn serve_loop(
    cfg: &Config,
    store: &nearbee_core::credentials::Store,
    opts: &nearbee_core::serve::ServeOptions,
    env: &Env<'_>,
    out: &mut Lines<'_>,
    rounds: Option<u32>,
) -> i32 {
    // The resolved listing records are read again at the start of every
    // round, so a listing republished while serve runs is taken for the
    // next new hire without a restart. A hire already started keeps the
    // version its journal recorded (round 6, codex 2). The per-run budget
    // is computed per hire from its ceiling (design 8), not fixed here.
    let load =
        || nearbee_core::serve::load_contexts(&cfg.state_dir, store, &cfg.apb.workspace_path);
    let (mut contexts, warnings) = match load() {
        Ok(pair) => pair,
        Err(e) => {
            out.line(&format!("error: listings: {e}"));
            return EXIT_ERROR;
        }
    };
    for w in &warnings {
        out.line(&format!("warning: {w}"));
    }
    // No listing with credentials loads: the journals on disk may still
    // owe local work (an active run to stop at its wall clock), which
    // needs neither a listing nor a token (R4-7). The rounds run until
    // that work is settled; only then does serve exit.
    let nothing_to_serve = "no published listing with credentials; nothing to serve";
    if contexts.is_empty() {
        if !nearbee_core::serve::local_work_outstanding(&cfg.state_dir) {
            out.line(nothing_to_serve);
            return 1;
        }
        out.line(
            "no published listing with credentials; serving the local work of the journaled hires until it settles",
        );
    }
    // Warnings are printed when they change, not on every round.
    let mut last_warnings = warnings;
    // Agents whose token was revoked are removed from the polling after
    // the operator is told once.
    let mut stopped: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut round: u32 = 0;
    let mut round_state = nearbee_core::serve::RoundState::default();
    let clock = nearbee_core::serve::Clock::real();
    let interval = std::time::Duration::from_secs(opts.poll_interval_secs);
    loop {
        // The cadence is counted from each round's start (spec
        // `hire-serving`, Polling; C14).
        let round_started = std::time::Instant::now();
        if round > 0 {
            match load() {
                Ok((fresh, warnings)) => {
                    if warnings != last_warnings {
                        for w in &warnings {
                            out.line(&format!("warning: {w}"));
                        }
                        last_warnings = warnings;
                    }
                    contexts = fresh;
                }
                Err(e) => {
                    // The previous records stay in use; the next round
                    // reads them again.
                    out.line(&format!(
                        "warning: listings: {e}; the previous records are kept"
                    ));
                }
            }
        }
        // One round over every served agent's rows (design 1): the reads
        // happen per agent here, the machine runs once for all of them.
        // Every agent keeps its own market client: its hires' writes go
        // out with its own token.
        let mut reads: Vec<(
            nearbee_core::serve::ListingContext,
            Vec<nearbee_core::market::Row>,
            NearAgentMarket,
        )> = Vec::new();
        // apb down never skips the round: the machine still runs its
        // journal-driven and time-based steps over a runner whose every
        // call fails, and asks nothing and starts nothing (O3).
        let runner = match ApbRunner::connect(apb_options(cfg, env), &cfg.apb.workspace_path).await
        {
            Ok(r) => RoundRunner::Up(Box::new(r)),
            Err(ConnectError::Runner(e)) => {
                out.line(&format!("error: apb: {e}"));
                RoundRunner::down(e.to_string())
            }
            Err(ConnectError::Workspace(e)) => {
                out.line(&format!("error: apb workspace: {e}"));
                RoundRunner::down(e.to_string())
            }
        };
        round_state.runner_unavailable = matches!(runner, RoundRunner::Down { .. });
        {
            for (handle, ctx) in &contexts {
                if stopped.contains(handle) {
                    continue;
                }
                let creds = match store.get(handle) {
                    Ok(Some(c)) => c,
                    Ok(None) => continue,
                    Err(e) => {
                        out.line(&format!("warning: {handle}: credentials: {e}"));
                        continue;
                    }
                };
                let market = match NearAgentMarket::new(market_options(
                    market_url(env),
                    creds.token.clone(),
                    cfg.secrets(),
                )) {
                    Ok(c) => c,
                    Err(e) => {
                        out.line(&format!("error: market for {handle}: {e}"));
                        continue;
                    }
                };
                match market.assignments().await {
                    Ok(rows) => reads.push((ctx.clone(), rows, market)),
                    Err(nearbee_core::market::MarketError::Unauthorized { .. }) => {
                        stopped.insert(handle.clone());
                        out.line(&format!(
                            "unauthorized: the token of {handle} was revoked; {handle} is no longer polled, the others keep serving"
                        ));
                    }
                    Err(e) => {
                        out.line(&format!("error: {handle}: {e}"));
                    }
                }
            }
            // The machine runs even when no read succeeded this round (every
            // agent's read failed or every token is revoked): the hires on
            // disk are then judged from their journals alone, so an active
            // run is still observed and stopped at its wall clock (R2-2).
            {
                let agents: Vec<nearbee_core::serve::Agent<'_, NearAgentMarket>> = reads
                    .iter()
                    .map(|(ctx, rows, market)| nearbee_core::serve::Agent {
                        ctx: ctx.clone(),
                        rows: rows.clone(),
                        market,
                    })
                    .collect();
                let attestor = serve_attestor(cfg, env, agents.iter().map(|a| &a.ctx));
                match nearbee_core::serve::round(
                    &runner,
                    &attestor,
                    &agents,
                    &mut round_state,
                    opts,
                    &cfg.state_dir,
                    &cfg.limits,
                    &clock,
                )
                .await
                {
                    Ok(outcomes) => {
                        for o in outcomes {
                            out.line(&format!("{}: {o:?}", o.row_id()));
                        }
                    }
                    Err(e) => {
                        out.line(&format!("error: the round failed: {e}"));
                    }
                }
            }
        }
        round += 1;
        if contexts.is_empty() && !nearbee_core::serve::local_work_outstanding(&cfg.state_dir) {
            out.line(&format!("{nothing_to_serve}: the local work is settled"));
            return 1;
        }
        if rounds.is_some_and(|r| round >= r) {
            return 0;
        }
        tokio::time::sleep(time_to_next_round(interval, round_started.elapsed())).await;
    }
}

/// How long to wait before the next round: the rest of the interval
/// counted from this round's start; a round that took longer than the
/// interval is followed at once (C14).
fn time_to_next_round(
    interval: std::time::Duration,
    elapsed: std::time::Duration,
) -> std::time::Duration {
    interval.saturating_sub(elapsed)
}

/// The runner of one serve round: apb, or (when apb cannot be reached
/// this round) a stand-in whose every call fails, so the round still runs
/// without it (O3).
enum RoundRunner {
    Up(Box<ApbRunner>),
    Down {
        workspace: nearbee_core::runner::WorkspaceId,
        why: String,
    },
}

impl RoundRunner {
    fn down(why: String) -> Self {
        RoundRunner::Down {
            workspace: nearbee_core::runner::WorkspaceId::new(""),
            why,
        }
    }

    fn unavailable(&self) -> nearbee_core::runner::RunnerError {
        let why = match self {
            RoundRunner::Down { why, .. } => why.clone(),
            RoundRunner::Up(_) => String::new(),
        };
        nearbee_core::runner::RunnerError::RetryableRead {
            detail: format!("apb cannot be reached this round: {why}"),
        }
    }
}

impl Runner for RoundRunner {
    fn workspace(&self) -> &nearbee_core::runner::WorkspaceId {
        match self {
            RoundRunner::Up(r) => r.workspace(),
            RoundRunner::Down { workspace, .. } => workspace,
        }
    }
    async fn playbooks(
        &self,
    ) -> Result<Vec<nearbee_core::runner::PlaybookSummary>, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.playbooks().await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn estimate(
        &self,
        id: &PlaybookId,
    ) -> Result<nearbee_core::runner::Estimate, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.estimate(id).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn start(
        &self,
        req: nearbee_core::runner::StartRequest,
    ) -> Result<nearbee_core::runner::RunId, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.start(req).await,
            // Never reached: no start is admitted while apb is down.
            RoundRunner::Down { .. } => Err(nearbee_core::runner::RunnerError::Refused {
                status: 503,
                code: "apb_unavailable".into(),
                detail: "apb cannot be reached this round".into(),
                estimate: None,
            }),
        }
    }
    async fn external_data_supported(&self) -> Result<bool, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.external_data_supported().await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn run(
        &self,
        id: &nearbee_core::runner::RunId,
    ) -> Result<nearbee_core::runner::RunDetail, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.run(id).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn stop(
        &self,
        id: &nearbee_core::runner::RunId,
    ) -> Result<(), nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.stop(id).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn abort(
        &self,
        id: &nearbee_core::runner::RunId,
    ) -> Result<(), nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.abort(id).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn decide(
        &self,
        question: &str,
        criteria: &str,
        limit: std::time::Duration,
    ) -> Result<nearbee_core::runner::DecideOutcome, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.decide(question, criteria, limit).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn find(
        &self,
        q: RunQuery,
    ) -> Result<nearbee_core::runner::RunLookup, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.find(q).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn inspect(
        &self,
        id: &PlaybookId,
    ) -> Result<nearbee_core::runner::ExecutionFacts, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.inspect(id).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
    async fn inspect_source(
        &self,
        source: &str,
    ) -> Result<nearbee_core::runner::ExecutionFacts, nearbee_core::runner::RunnerError> {
        match self {
            RoundRunner::Up(r) => r.inspect_source(source).await,
            RoundRunner::Down { .. } => Err(self.unavailable()),
        }
    }
}

/// `nearbee hires [--json]`: one line per journaled hire, read only and
/// without the lock (spec `hire-retention`). No brief text, token or
/// capability link is printed.
pub fn hires(file: Option<&Path>, args: HiresArgs, env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut out = Lines::new(out);
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            out.line(&format!("error: config: {e}"));
            return EXIT_ERROR;
        }
    };
    out.secrets = cfg.secrets();
    let all = nearbee_core::retention::summaries(&cfg.state_dir);
    if args.json {
        out.line(&serde_json::to_string_pretty(&all).expect("summaries serialize"));
        return 0;
    }
    if all.is_empty() {
        out.line(&format!(
            "no journaled hires in {}",
            cfg.state_dir.join("hires").display()
        ));
        return 0;
    }
    out.line("hire	agent	state	latest run	spent USD	deadline	attention");
    for s in &all {
        let dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
        let attention = match (&s.problem, &s.attention) {
            (Some(p), _) => format!("unreadable: {p}"),
            (None, a) => dash(a),
        };
        out.line(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            s.hire_id,
            dash(&s.handle),
            s.state,
            dash(&s.latest_run),
            s.spent_usd,
            dash(&s.deadline),
            attention
        ));
    }
    0
}

/// `nearbee cleanup [--dry-run]`: deletes the files of eligible hires
/// (their worktrees and snapshot directories) under the serve lock and
/// journals what was removed; reports what Nearbee cannot delete.
pub fn cleanup(file: Option<&Path>, args: CleanupArgs, env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut out = Lines::new(out);
    let cfg = match load(file, env) {
        Ok(c) => c,
        Err(e) => {
            out.line(&format!("error: config: {e}"));
            return EXIT_ERROR;
        }
    };
    out.secrets = cfg.secrets();
    // The dry run takes no lock and changes nothing.
    let _lock = if args.dry_run {
        None
    } else {
        match nearbee_core::lock::acquire(&cfg.state_dir) {
            Ok(l) => Some(l),
            Err(e @ nearbee_core::lock::LockError::Held { .. }) => {
                out.line(&format!("error: {e}"));
                return nearbee_core::lock::EXIT_LOCKED;
            }
            Err(e) => {
                out.line(&format!("error: {e}"));
                return EXIT_ERROR;
            }
        }
    };
    let plans = nearbee_core::retention::plan(
        &cfg.state_dir,
        &cfg.apb.workspace_path,
        cfg.retention.after_acceptance_days,
        time::OffsetDateTime::now_utc(),
    );
    let mut failed = false;
    for p in &plans {
        if !p.eligible {
            out.line(&format!("keep {}: {}", p.hire_id, p.why));
            continue;
        }
        if args.dry_run {
            out.line(&format!("would delete {} ({})", p.hire_id, p.why));
            for d in &p.delete {
                out.line(&format!("  delete {}", d.display()));
            }
            for r in &p.refused {
                out.line(&format!(
                    "  refused {} (outside the workspace's work directory)",
                    r.display()
                ));
            }
            for n in &p.not_covered {
                out.line(&format!("  not covered: {n}"));
            }
            for u in &p.unlisted {
                out.line(&format!("  {u}"));
            }
            continue;
        }
        match nearbee_core::retention::execute(p) {
            Ok(lines) => {
                out.line(&format!("deleted {} ({})", p.hire_id, p.why));
                for l in lines {
                    if l.starts_with("failed ") {
                        failed = true;
                    }
                    out.line(&format!("  {l}"));
                }
            }
            Err(e) => {
                failed = true;
                out.line(&format!("error: {}: {e}", p.hire_id));
            }
        }
    }
    i32::from(failed)
}

/// `nearbee agent list`: handles only, tokens masked.
pub fn agent_list(env: &Env<'_>, out: &mut dyn Write) -> i32 {
    let mut out = Lines::new(out);
    let Some(store) = store(env) else {
        out.line("error: cannot find a credentials directory (HOME is unset)");
        return EXIT_ERROR;
    };
    match store.read() {
        Err(CredentialsError::Read { path, source })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            out.line(&format!("no agents stored ({})", path.display()));
            1
        }
        Err(e) => {
            out.line(&format!("error: {}: {e}", store.path().display()));
            EXIT_ERROR
        }
        Ok(all) => {
            for (handle, creds) in &all {
                out.line(&format!(
                    "{handle}\t{}\t{}",
                    creds.agent_id,
                    nearbee_core::credentials::masked(&creds.token)
                ));
            }
            i32::from(all.is_empty())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nearbee_core::evidence::Attestor;

    fn config(vars: &[(&str, &str)]) -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_string_lossy().to_string();
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .chain([("HOME".to_owned(), home)])
            .collect();
        let cfg = config::load(
            LoadOptions {
                file: None,
                cwd: dir.path(),
            },
            |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
        )
        .unwrap();
        (dir, cfg)
    }

    fn listing(tier: nearbee_core::listing::Tier) -> nearbee_core::serve::ListingContext {
        let record: nearbee_core::listing::ResolvedListing =
            serde_json::from_value(serde_json::json!({
                "handle": "h",
                "agent_id": "a",
                "playbook_id": "p",
                "playbook_version": "1.0.0",
                "playbook_sha256": "ab",
                "price_usd": "0.50",
                "sla_seconds": 3600,
                "tier": tier,
                "published_at": "2026-11-11T00:00:00Z"
            }))
            .unwrap();
        nearbee_core::serve::ListingContext {
            handle: "h".into(),
            agent_id: "a".into(),
            playbook: nearbee_core::runner::PlaybookId::new("p"),
            workspace: std::path::PathBuf::from("/ws"),
            price_usd: rust_decimal::Decimal::new(50, 2),
            scope: None,
            protection: nearbee_core::listing::Protection::On,
            limits: None,
            sla_seconds: Some(3600),
            playbook_version: "1.0.0".into(),
            playbook_sha256: "ab".into(),
            record: Some(record),
        }
    }

    fn env_of(dir: &std::path::Path) -> (std::path::PathBuf, impl Fn(&str) -> Option<String>) {
        (dir.to_path_buf(), |_: &str| None)
    }

    /// No `NEARAI_API_KEY` with an attested listing served: every fetch
    /// fails with that reason and nothing is sent (spec `run-evidence`,
    /// No key); plain listings never need the key.
    #[tokio::test]
    async fn the_serve_attestor_needs_the_key_only_for_attested_listings() {
        use nearbee_core::listing::Tier;
        let (dir, cfg) = config(&[]);
        let (cwd, var) = env_of(dir.path());
        let env = Env {
            cwd: &cwd,
            var: &var,
            timing: Timing::default(),
        };
        let attested = [listing(Tier::Attested)];
        let a = serve_attestor(&cfg, &env, attested.iter());
        assert!(matches!(a, ServeAttestor::Off(_)));
        assert_eq!(
            a.report("Qwen/Qwen3.8-27B", "00").await.unwrap_err(),
            nearbee_cloud::NO_KEY
        );
        let plain = [listing(Tier::Plain)];
        assert!(matches!(
            serve_attestor(&cfg, &env, plain.iter()),
            ServeAttestor::Off(_)
        ));
        let (dir, cfg) = config(&[("NEARAI_API_KEY", "sk-test-0123456789")]);
        let (cwd, var) = env_of(dir.path());
        let env = Env {
            cwd: &cwd,
            var: &var,
            timing: Timing::default(),
        };
        assert!(matches!(
            serve_attestor(&cfg, &env, attested.iter()),
            ServeAttestor::Cloud(_)
        ));
    }
}
