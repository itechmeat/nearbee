//! The `nearbee` binary.

use clap::Parser;
use nearbee::{Cli, Env};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("NEARBEE_LOG"))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let var = |k: &str| std::env::var(k).ok();
    let env = Env {
        cwd: &cwd,
        var: &var,
        timing: Default::default(),
    };
    let mut out = std::io::stdout();
    let code = nearbee::run(cli, &env, &mut out).await;
    std::process::exit(code);
}
