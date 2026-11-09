//! The `nearbee` command line: `config check` and `runs find`.
//!
//! The commands write to the given writer and return the process exit code,
//! so tests drive them without spawning the binary.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use nearbee_apb::{ApbClient, ApbOptions, ApbRunner, ConnectError, Timing, resolve_workspace};
use nearbee_core::config::{self, Config, LoadOptions, VARIABLES};
use nearbee_core::redact::redact_detail;
use nearbee_core::runner::{ExecutionId, HireId, PlaybookId, RunQuery, Runner, SearchField};
use nearbee_core::secret::Secret;

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
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Load the configuration, report variables by name, reach apb and
    /// resolve the workspace.
    Check,
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
struct Lines<'a> {
    out: &'a mut dyn Write,
    secrets: Vec<Secret>,
}

impl<'a> Lines<'a> {
    fn new(out: &'a mut dyn Write) -> Self {
        Self {
            out,
            secrets: Vec::new(),
        }
    }

    fn line(&mut self, text: &str) {
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
