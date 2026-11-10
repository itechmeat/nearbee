//! The process lock of `serve` and `cleanup` (design 12): an exclusive,
//! non-blocking `flock` on `<state_dir>/serve.lock`, held for the life of
//! the process. The kernel drops it with the process, so a crash leaves
//! no stale lock; the file itself stays (it is never unlinked, so two
//! processes can never lock two different files of the same name).
//!
//! The file is opened close-on-exec (the Rust standard library opens every
//! file with `O_CLOEXEC`), so the children serve spawns (apb, decide) do
//! not inherit the lock and cannot keep it after serve ends.

use std::path::{Path, PathBuf};

/// The exit code of a command that found the lock held.
pub const EXIT_LOCKED: i32 = 4;

/// The lock file's name inside the state directory.
pub const LOCK_FILE: &str = "serve.lock";

/// Why the lock was not taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process holds the lock.
    #[error("another nearbee serve or cleanup holds {}", path.display())]
    Held { path: PathBuf },
    /// The lock file cannot be opened or locked.
    #[error("cannot lock {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A held lock; dropping it releases the lock.
#[derive(Debug)]
pub struct ServeLock {
    path: PathBuf,
    _file: std::fs::File,
}

impl ServeLock {
    /// The lock file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The lock path of a state directory.
pub fn lock_path(state_dir: &Path) -> PathBuf {
    state_dir.join(LOCK_FILE)
}

/// Takes the exclusive lock of `state_dir` without waiting: create the file
/// (mode 0600, no truncate), lock it, then truncate it and write the pid.
pub fn acquire(state_dir: &Path) -> Result<ServeLock, LockError> {
    let path = lock_path(state_dir);
    let io = |source: std::io::Error| LockError::Io {
        path: path.clone(),
        source,
    };
    std::fs::create_dir_all(state_dir).map_err(io)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(io)?;
    lock_exclusive(&file).map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            LockError::Held { path: path.clone() }
        } else {
            io(e)
        }
    })?;
    // The pid is for the operator only; the lock itself is the flock.
    {
        use std::io::Write;
        file.set_len(0).map_err(io)?;
        let mut f = &file;
        writeln!(f, "{}", std::process::id()).map_err(io)?;
        let _ = f.flush();
    }
    Ok(ServeLock { path, _file: file })
}

#[cfg(unix)]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(std::io::Error::from)
}

#[cfg(not(unix))]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    file.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => std::io::ErrorKind::WouldBlock.into(),
        std::fs::TryLockError::Error(e) => e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lock tests run one at a time: a fork in a parallel test copies
    /// every descriptor of this process until its exec, which would hold
    /// another test's lock for that instant.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes the lock again after a release, tolerating the instant a
    /// fork of another test in this process (between its fork and its
    /// exec) still shares the descriptor; a real holder (the 30 s child)
    /// keeps it far longer than this one second.
    fn acquire_soon(dir: &Path) -> Result<ServeLock, LockError> {
        let mut last = None;
        for _ in 0..100 {
            match acquire(dir) {
                Ok(l) => return Ok(l),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(last.expect("at least one attempt"))
    }

    #[test]
    fn a_second_lock_attempt_fails_while_the_first_is_held() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let first = acquire(dir.path()).expect("first lock");
        let second = acquire(dir.path());
        match second {
            Err(LockError::Held { path }) => {
                assert_eq!(path, lock_path(dir.path()));
                let msg = LockError::Held { path }.to_string();
                assert!(msg.starts_with("another nearbee serve or cleanup holds "));
                assert!(msg.ends_with("serve.lock"));
            }
            other => panic!("expected Held, got {other:?}"),
        }
        drop(first);
        acquire_soon(dir.path()).expect("the lock is free again after the drop");
    }

    #[test]
    fn the_lock_file_is_private_and_carries_the_pid() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let lock = acquire(dir.path()).unwrap();
        let text = std::fs::read_to_string(lock.path()).unwrap();
        assert_eq!(text.trim(), std::process::id().to_string());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(lock.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_child_process_does_not_inherit_the_lock() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let lock = acquire(dir.path()).unwrap();
        // A child that outlives the parent's lock: if it inherited the
        // descriptor, the lock would stay held after the drop.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("sleep spawns");
        drop(lock);
        let again = acquire_soon(dir.path());
        let _ = child.kill();
        let _ = child.wait();
        again.expect("the child holds no copy of the lock");
    }
}
