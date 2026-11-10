//! The apb CLI calls (abort, decide): a child `apb` run from the
//! workspace, with the instance's `APB_CONFIG_DIR`, each argument passed
//! directly (no shell), the pipes drained while the child runs so a big
//! answer can never block it, and a hard kill at the timeout.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// One finished CLI run.
pub struct CliRun {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Why a CLI call failed before an answer existed.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("cannot run {bin}: {source}", bin = bin.display())]
    Spawn {
        bin: PathBuf,
        source: std::io::Error,
    },
    #[error("`apb {first}` did not finish within {secs} s and was killed", first = first)]
    Timeout { first: String, secs: u64 },
}

/// Runs the CLI. `remove` names environment variables the child must not
/// see (a decide outside any run carries no run or node id).
pub fn run(
    bin: &Path,
    args: &[&str],
    workspace: &Path,
    config_dir: &Path,
    remove: &[&str],
    timeout: Duration,
) -> Result<CliRun, CliError> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .current_dir(workspace)
        .env("APB_CONFIG_DIR", config_dir)
        .env_remove("NO_COLOR");
    for name in remove {
        cmd.env_remove(name);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| CliError::Spawn {
            bin: bin.to_path_buf(),
            source,
        })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st.code().unwrap_or(-1)),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CliError::Timeout {
                        first: args.first().copied().unwrap_or_default().to_owned(),
                        secs: timeout.as_secs(),
                    });
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break Some(-1),
        }
    };
    let stdout = stdout.join().map_err(|_| CliError::Spawn {
        bin: bin.to_path_buf(),
        source: std::io::Error::other("the output reader failed"),
    })?;
    let stderr = stderr.join().map_err(|_| CliError::Spawn {
        bin: bin.to_path_buf(),
        source: std::io::Error::other("the output reader failed"),
    })?;
    Ok(CliRun {
        status,
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

/// Reads a child's pipe to the end on its own thread, so the child can
/// never block on a full pipe buffer while the parent waits for its exit.
fn drain<R: std::io::Read + Send + 'static>(mut p: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = p.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}
