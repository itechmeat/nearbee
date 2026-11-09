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

pub mod publish;

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
fn apb_bin(env: &Env<'_>) -> PathBuf {
    (env.var)("APB_BIN")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("apb"))
}

/// The apb instance's config directory: `APB_CONFIG_DIR`, default the
/// Nearbee instance's directory (as `scripts/apb-instance.sh` creates it).
fn apb_config_dir(env: &Env<'_>) -> Result<PathBuf, String> {
    if let Some(p) = (env.var)("APB_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let home = (env.var)("HOME").filter(|v| !v.is_empty()).ok_or_else(|| {
        "cannot find the apb config directory (set APB_CONFIG_DIR or HOME)".to_string()
    })?;
    Ok(Path::new(&home)
        .join(".local")
        .join("share")
        .join("nearbee")
        .join("apb-config"))
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
    let config_dir = match apb_config_dir(env) {
        Ok(p) => p,
        Err(e) => {
            lines.line(&format!("error: {e}"));
            return EXIT_ERROR;
        }
    };
    let pub_env = publish::PublishEnv {
        market_url: market_url(env),
        apb_bin: apb_bin(env),
        apb_config_dir: config_dir,
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
    let poll = (env.var)("NEARBEE_POLL_SECS")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(nearbee_core::serve::POLL_INTERVAL_MIN_SECS);
    let opts = nearbee_core::serve::ServeOptions {
        reject_handle: args.reject.clone(),
        // The poll interval is validated to the spec: every 30 to 60 s.
        poll_interval_secs: nearbee_core::serve::clamp_poll_interval_secs(poll),
        forbidden: forbidden_secrets(&cfg, &store),
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
    use nearbee_core::runner::{Budget, OnNoEstimate};
    // The resolved listing records are read again at the start of every
    // round, so a listing republished while serve runs is taken for the
    // next new hire without a restart. A hire already started keeps the
    // version its journal recorded (round 6, codex 2).
    let load = || {
        nearbee_core::serve::load_contexts(
            &cfg.state_dir,
            store,
            &cfg.apb.workspace_path,
            Budget {
                usd: None,
                on_no_estimate: OnNoEstimate::Continue,
            },
        )
    };
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
    if contexts.is_empty() {
        out.line("no published listing with credentials; nothing to serve");
        return 1;
    }
    // Warnings are printed when they change, not on every round.
    let mut last_warnings = warnings;
    // Agents whose token was revoked are removed from the polling after
    // the operator is told once.
    let mut stopped: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut round: u32 = 0;
    loop {
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
        for (handle, ctx) in &contexts {
            if stopped.contains(handle) {
                continue;
            }
            let creds = match store.get(handle) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => {
                    // A failed credentials read is reported, not skipped
                    // silently: the mode or the file needs an operator.
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
            let runner =
                match ApbRunner::connect(apb_options(cfg, env), &cfg.apb.workspace_path).await {
                    Ok(r) => r,
                    Err(ConnectError::Runner(e)) => {
                        out.line(&format!("error: apb for {handle}: {e}"));
                        continue;
                    }
                    Err(ConnectError::Workspace(e)) => {
                        // One broken workspace must not stop the other
                        // agents; it is reported and the round goes on.
                        out.line(&format!("error: apb workspace: {e}"));
                        continue;
                    }
                };
            match nearbee_core::serve::poll_agent(
                &market,
                &runner,
                ctx,
                opts,
                &cfg.state_dir,
                &cfg.limits,
            )
            .await
            {
                Ok(outcomes) => {
                    for o in outcomes {
                        out.line(&format!("{handle} {}: {o:?}", o.row_id()));
                    }
                }
                Err(nearbee_core::serve::ServeError::Market(
                    nearbee_core::market::MarketError::Unauthorized { .. },
                )) => {
                    // Revoked token: stop polling this agent only and tell
                    // the operator once.
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
        round += 1;
        if rounds.is_some_and(|r| round >= r) {
            return 0;
        }
        tokio::time::sleep(std::time::Duration::from_secs(opts.poll_interval_secs)).await;
    }
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
