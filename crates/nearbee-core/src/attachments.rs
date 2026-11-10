//! Attachments (spec `hire-attachments`, design 10): the files a buyer
//! attached to the job or to a rework message, brought into the hire's
//! worktree safely.
//!
//! The plan lists the files of a generation (the job's attachments, then
//! the newest buyer message's for a rework), skips ids an `inputs` record
//! already holds, and gives each new id the next index of the hire, so a
//! path never changes and two reworks never share one. A download goes to
//! `inputs/.<nn>.partial` through a SHA-256 hasher with a cap of
//! `byteSize + 1`, is verified (length and hash), fsynced and renamed to
//! `inputs/<nn>-<safe name>`. Leftover `*.partial` files are removed at the
//! start of every attempt.

use std::collections::BTreeSet;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::journal::{Event, InputFile, Record};
use crate::listing::ListingLimits;
use crate::market::{Attachment, Market, MarketError, Row};

/// The longest safe name, in bytes.
pub const SAFE_NAME_MAX: usize = 100;

/// The directory of the stored files inside the hire worktree.
pub const INPUTS_DIR: &str = "inputs";

/// One file to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub attachment_id: String,
    pub byte_size: u64,
    pub sha256: String,
    pub download_url: String,
    /// The index in the hire, given once.
    pub index: usize,
    /// `inputs/<nn>-<safe name>`, relative to the worktree.
    pub rel_path: String,
}

/// Why a generation cannot take its files: it is declined `failed` with a
/// generic reason (the detail goes to the journal only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub detail: String,
}

/// The safe form of a file name: `[A-Za-z0-9._-]` kept, every other
/// character replaced with `_`, leading dots dropped, at most 100 bytes;
/// an empty result becomes `file`.
pub fn safe_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = replaced.trim_start_matches('.');
    let mut out: String = trimmed.chars().take(SAFE_NAME_MAX).collect();
    // Every kept character is ASCII, so the char cut is a byte cut.
    out.truncate(SAFE_NAME_MAX);
    if out.is_empty() { "file".into() } else { out }
}

/// The files every `inputs` record of the hire holds, in index order.
pub fn stored(records: &[Record]) -> Vec<InputFile> {
    let mut out = Vec::new();
    for r in records {
        if let Event::Inputs(i) = &r.event {
            out.extend(i.files.iter().cloned());
        }
    }
    out
}

/// The `inputs` param: a JSON array of the stored paths in index order.
pub fn inputs_param(records: &[Record]) -> String {
    let paths: Vec<String> = stored(records).into_iter().map(|f| f.path).collect();
    serde_json::to_string(&paths).expect("a list of strings serializes")
}

/// The attachments a generation brings: the job's, then (for a rework)
/// the newest buyer message's.
pub fn sources(row: &Row, rework: bool) -> Vec<Attachment> {
    let mut out = row.job_attachments.clone();
    if rework
        && let Some(m) = &row.latest_message
        && m.sender_side.as_deref() == Some("buyer")
    {
        out.extend(m.attachments.iter().cloned());
    }
    out
}

/// Plans the files of a generation. Ids an `inputs` record holds are
/// skipped; each new id gets the next index of the hire. A declared size
/// that is negative or above `file_bytes` refuses the generation, and so
/// does a new file that breaks the listing's hard limits (security F1):
/// a type outside `file_types`, or the hire's stored plus new files over
/// `max_attachments` or `max_attachment_bytes`. Intake checks the job's
/// files before generation 1; this check is what covers a rework's.
pub fn plan(
    row: &Row,
    rework: bool,
    records: &[Record],
    file_bytes: u64,
    limits: Option<&ListingLimits>,
) -> Result<Vec<Planned>, Refusal> {
    let done = stored(records);
    let mut seen: BTreeSet<String> = done.iter().map(|f| f.attachment_id.clone()).collect();
    let mut next = done.len();
    let mut out = Vec::new();
    for a in sources(row, rework) {
        if !seen.insert(a.id.clone()) {
            continue;
        }
        if a.byte_size < 0 || a.byte_size as u64 > file_bytes {
            return Err(Refusal {
                detail: format!(
                    "attachment {} declares {} bytes; allowed 0 to {file_bytes}",
                    a.id, a.byte_size
                ),
            });
        }
        out.push(Planned {
            attachment_id: a.id.clone(),
            byte_size: a.byte_size as u64,
            sha256: a.sha256.to_ascii_lowercase(),
            download_url: a.download_url.clone(),
            index: next,
            rel_path: format!("{INPUTS_DIR}/{next:02}-{}", safe_name(&a.filename)),
        });
        next += 1;
    }
    if let Some(l) = limits {
        check_limits(l, &done, &out, row, rework)?;
    }
    Ok(out)
}

/// The listing's hard limits over the hire's stored and planned files.
fn check_limits(
    l: &ListingLimits,
    done: &[InputFile],
    planned: &[Planned],
    row: &Row,
    rework: bool,
) -> Result<(), Refusal> {
    let refuse = |detail: String| Err(Refusal { detail });
    if let Some(types) = &l.file_types {
        let names: Vec<(String, String)> = sources(row, rework)
            .into_iter()
            .map(|a| (a.id, a.filename))
            .collect();
        for p in planned {
            let name = names
                .iter()
                .find(|(id, _)| *id == p.attachment_id)
                .map(|(_, n)| n.as_str())
                .unwrap_or_default();
            if !crate::intake::file_type_allowed(name, types) {
                return refuse(format!(
                    "attachment {} has a file type outside file_types",
                    p.attachment_id
                ));
            }
        }
    }
    let count = done.len() + planned.len();
    if let Some(max) = l.max_attachments
        && count > max
    {
        return refuse(format!(
            "the hire would hold {count} attachments, over max_attachments {max}"
        ));
    }
    let bytes: u64 = done.iter().map(|f| f.bytes).sum::<u64>()
        + planned.iter().map(|p| p.byte_size).sum::<u64>();
    if let Some(max) = l.max_attachment_bytes
        && bytes > max
    {
        return refuse(format!(
            "the hire's attachments would declare {bytes} bytes, over max_attachment_bytes {max}"
        ));
    }
    Ok(())
}

/// The origin check before any request (the client checks again): an
/// absolute URL must be on the market origin (same scheme, host and port)
/// without userinfo or a fragment; a relative path resolves against the
/// origin. Returns the absolute URL.
pub fn check_origin(origin: &str, download_url: &str) -> Result<String, Refusal> {
    let refuse = |d: &str| Refusal { detail: d.into() };
    let base = url::Url::parse(origin).map_err(|_| refuse("the market origin does not parse"))?;
    let url = if download_url.starts_with("http://") || download_url.starts_with("https://") {
        url::Url::parse(download_url).map_err(|_| refuse("the download url does not parse"))?
    } else if download_url.starts_with('/') {
        base.join(download_url)
            .map_err(|_| refuse("the download path does not join the origin"))?
    } else {
        return Err(refuse(
            "the download url is neither on the origin nor a path",
        ));
    };
    let same = url.scheme() == base.scheme()
        && url.host() == base.host()
        && url.port_or_known_default() == base.port_or_known_default();
    if !same {
        return Err(refuse("the download url is not on the market origin"));
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(refuse("the download url carries userinfo or a fragment"));
    }
    Ok(url.to_string())
}

/// The outcome of one download attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    /// Verified and stored (or already on disk and verified).
    Stored(InputFile),
    /// Retry in a later round, no sooner than this many seconds.
    Later { after_secs: u64 },
    /// The generation is declined `failed`.
    Refused(Refusal),
    /// The token was refused: stop the agent.
    Unauthorized,
}

fn partial_name(index: usize) -> String {
    format!(".{index:02}.partial")
}

/// The file name a planned path has inside `inputs/`.
fn stored_name(p: &Planned) -> &str {
    p.rel_path
        .strip_prefix(INPUTS_DIR)
        .and_then(|r| r.strip_prefix('/'))
        .unwrap_or(&p.rel_path)
}

/// The hire worktree's `inputs` directory, opened without following a
/// symlink at the worktree's own name or at `inputs` (C12): every file of
/// the hire is then created, renamed and removed relative to this handle,
/// so a symlink an earlier generation left can never send a write out of
/// the worktree. `create` makes `inputs` first when it is missing.
fn open_inputs(wt: &Path, create: bool) -> std::io::Result<OwnedFd> {
    use rustix::fs::{CWD, Mode, OFlags, mkdirat, openat};
    let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a worktree path");
    let parent = wt.parent().ok_or_else(invalid)?;
    let name = wt.file_name().ok_or_else(invalid)?;
    let dir = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let parent_fd = openat(CWD, parent, dir, Mode::empty())?;
    let wt_fd = openat(&parent_fd, name, dir | OFlags::NOFOLLOW, Mode::empty())?;
    if create {
        match mkdirat(&wt_fd, INPUTS_DIR, Mode::from_raw_mode(0o700)) {
            // The `inputs` entry is durable before any file in it can be
            // journaled (R2-11): its parent, the worktree, is synced. An
            // `inputs` that already exists may be the leftover of a crash
            // between its `mkdirat` and that sync, so it passes the same
            // barrier (R4-6).
            Ok(()) | Err(rustix::io::Errno::EXIST) => rustix::fs::fsync(&wt_fd)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(openat(
        &wt_fd,
        INPUTS_DIR,
        dir | OFlags::NOFOLLOW,
        Mode::empty(),
    )?)
}

/// Removes leftover `*.partial` files of earlier attempts.
pub fn clear_partials(wt: &Path) {
    let Ok(inputs) = open_inputs(wt, false) else {
        return;
    };
    clear_partials_in(&inputs);
}

fn clear_partials_in(inputs: &OwnedFd) {
    let Ok(dir) = rustix::fs::Dir::read_from(inputs) else {
        return;
    };
    let names: Vec<std::ffi::CString> = dir
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy();
            n.starts_with('.') && n.ends_with(".partial")
        })
        .map(|e| e.file_name().to_owned())
        .collect();
    for n in names {
        let _ = rustix::fs::unlinkat(inputs, n.as_c_str(), rustix::fs::AtFlags::empty());
    }
}

/// Whether a planned file is already stored and verified in the worktree
/// (a crash came after its rename and before its `inputs` record). A file
/// found this way is made durable before `true` is returned, exactly as a
/// fresh download is (R2-11, R3-6): its directory and the worktree that
/// holds `inputs` are synced, and a failed sync is an error, never a
/// stored file. A missing worktree or `inputs` is `false`.
pub fn recover_stored(wt: &Path, p: &Planned) -> std::io::Result<bool> {
    let inputs = match open_inputs(wt, false) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if !verified_on_disk(&inputs, stored_name(p), p) {
        return Ok(false);
    }
    sync_recovered(wt, &inputs)?;
    Ok(true)
}

/// The durability barrier of a file found already stored: the `inputs`
/// directory (its entry) and the worktree (the `inputs` entry, which a
/// crash may have left unsynced right after its `mkdirat`).
fn sync_recovered(wt: &Path, inputs: &OwnedFd) -> std::io::Result<()> {
    use rustix::fs::{CWD, Mode, OFlags, openat};
    rustix::fs::fsync(inputs)?;
    let wt_fd = openat(
        CWD,
        wt,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    rustix::fs::fsync(&wt_fd)?;
    Ok(())
}

/// A regular file (never a symlink) in `inputs/` with the planned length
/// and hash.
fn verified_on_disk(inputs: &OwnedFd, name: &str, p: &Planned) -> bool {
    verified_file(inputs, name, p.byte_size, &p.sha256)
}

/// A regular file in `inputs/` with this length and hash. The open never
/// blocks and never follows a symlink: a FIFO or a device left at the name
/// is not a stored file and cannot stall the round (R2-10); the read is
/// bounded by the expected length plus one byte.
fn verified_file(inputs: &OwnedFd, name: &str, byte_size: u64, sha256: &str) -> bool {
    use rustix::fs::{FileType, Mode, OFlags, fstat, openat};
    let Ok(fd) = openat(
        inputs,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return false;
    };
    match fstat(&fd) {
        Ok(st) if FileType::from_raw_mode(st.st_mode) == FileType::RegularFile => {}
        _ => return false,
    }
    let mut bytes = Vec::new();
    let mut file = std::io::Read::take(std::fs::File::from(fd), byte_size.saturating_add(1));
    if std::io::Read::read_to_end(&mut file, &mut bytes).is_err() {
        return false;
    }
    bytes.len() as u64 == byte_size && hex::encode(Sha256::digest(&bytes)) == sha256
}

/// The first file an `inputs` record holds that is missing from the
/// worktree or does not verify (a power cut can lose a file whose record
/// survived, R2-11); `None` when every stored file is in place.
pub fn missing_stored(wt: &Path, records: &[Record]) -> Option<String> {
    let files = stored(records);
    if files.is_empty() {
        return None;
    }
    let Ok(inputs) = open_inputs(wt, false) else {
        return files.first().map(|f| f.path.clone());
    };
    files
        .into_iter()
        .find(|f| {
            let name = f
                .path
                .strip_prefix(INPUTS_DIR)
                .and_then(|r| r.strip_prefix('/'))
                .unwrap_or(&f.path);
            !verified_file(&inputs, name, f.bytes, &f.sha256)
        })
        .map(|f| f.path)
}

/// A writer that hashes what it writes.
struct Hashing<W> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Downloads one planned file into the worktree, at most `timeout` long
/// (a cut-off attempt is retried later, never counted as a failure).
pub async fn fetch<M: Market>(
    market: &M,
    wt: &Path,
    p: &Planned,
    timeout: std::time::Duration,
) -> Fetch {
    let url = match check_origin(&market.origin(), &p.download_url) {
        Ok(u) => u,
        Err(r) => return Fetch::Refused(r),
    };
    let inputs = match open_inputs(wt, true) {
        Ok(d) => d,
        Err(e) => {
            return Fetch::Refused(Refusal {
                detail: format!(
                    "the inputs directory cannot be opened inside the hire worktree: {e}"
                ),
            });
        }
    };
    let name = stored_name(p).to_owned();
    let stored = InputFile {
        attachment_id: p.attachment_id.clone(),
        path: p.rel_path.clone(),
        bytes: p.byte_size,
        sha256: p.sha256.clone(),
    };
    // Verified before a crash that came before its `inputs` record: the
    // same barrier as `recover_stored`, so its entry is durable before the
    // record (R2-11, R3-6).
    if verified_on_disk(&inputs, &name, p) {
        if let Err(e) = sync_recovered(wt, &inputs) {
            return Fetch::Refused(Refusal {
                detail: format!("cannot sync the stored file's directories: {e}"),
            });
        }
        return Fetch::Stored(stored);
    }
    clear_partials_in(&inputs);
    let partial = partial_name(p.index);
    let file = match rustix::fs::openat(
        &inputs,
        partial.as_str(),
        rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::from_raw_mode(0o600),
    ) {
        Ok(fd) => std::fs::File::from(fd),
        Err(e) => {
            return Fetch::Refused(Refusal {
                detail: format!("cannot create the partial file: {e}"),
            });
        }
    };
    let mut w = Hashing {
        inner: file,
        hasher: Sha256::new(),
    };
    let cap = p.byte_size + 1;
    let result = tokio::time::timeout(timeout, market.download(&url, cap, &mut w)).await;
    let remove_partial = || {
        let _ = rustix::fs::unlinkat(&inputs, partial.as_str(), rustix::fs::AtFlags::empty());
    };
    let mismatch = |detail: String| {
        remove_partial();
        Fetch::Refused(Refusal { detail })
    };
    let n = match result {
        Err(_) => {
            remove_partial();
            return Fetch::Later { after_secs: 0 };
        }
        Ok(Err(MarketError::Unauthorized { .. })) => {
            remove_partial();
            return Fetch::Unauthorized;
        }
        Ok(Err(MarketError::RetryableRead {
            retry_after_secs, ..
        })) => {
            remove_partial();
            return Fetch::Later {
                after_secs: retry_after_secs.unwrap_or(30),
            };
        }
        Ok(Err(MarketError::Refused { status, .. })) => {
            return mismatch(format!("the download was refused (HTTP {status})"));
        }
        Ok(Err(e)) => return mismatch(format!("the download failed: {e}")),
        Ok(Ok(n)) => n,
    };
    let digest = hex::encode(w.hasher.finalize_reset());
    if n != p.byte_size {
        return mismatch(format!(
            "attachment {} streamed {n} bytes, declared {}",
            p.attachment_id, p.byte_size
        ));
    }
    if digest != p.sha256 {
        return mismatch(format!(
            "attachment {} does not hash to its sha256",
            p.attachment_id
        ));
    }
    if let Err(e) = w.inner.sync_all() {
        return mismatch(format!("cannot sync the file: {e}"));
    }
    drop(w);
    if let Err(e) = rustix::fs::renameat(&inputs, partial.as_str(), &inputs, name.as_str()) {
        return mismatch(format!("cannot store the file: {e}"));
    }
    // The rename is durable before success is reported and journaled
    // (R2-11): a failed directory sync is never ignored.
    if let Err(e) = rustix::fs::fsync(&inputs) {
        return mismatch(format!("cannot sync the inputs directory: {e}"));
    }
    Fetch::Stored(stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(body: &[u8]) -> Planned {
        Planned {
            attachment_id: "f1".into(),
            byte_size: body.len() as u64,
            sha256: hex::encode(Sha256::digest(body)),
            download_url: "https://market.test/f1".into(),
            index: 0,
            rel_path: format!("{INPUTS_DIR}/00-a.txt"),
        }
    }

    /// R3-6: a file a crash left stored is recovered through the same
    /// durability barrier as a download; anything that is not a verified
    /// regular file is not stored, and an `inputs` the barrier cannot
    /// open (a symlink) is an error, never a stored file.
    #[test]
    fn a_recovered_file_passes_the_durability_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("wt");
        let body = b"job file";
        let p = planned(body);
        assert!(!recover_stored(&wt, &p).unwrap(), "no worktree yet");
        std::fs::create_dir_all(wt.join(INPUTS_DIR)).unwrap();
        assert!(!recover_stored(&wt, &p).unwrap(), "no file yet");
        std::fs::write(wt.join(&p.rel_path), b"other bytes").unwrap();
        assert!(!recover_stored(&wt, &p).unwrap(), "a wrong file");
        std::fs::write(wt.join(&p.rel_path), body).unwrap();
        assert!(recover_stored(&wt, &p).unwrap());
        // `inputs` replaced by a symlink to a directory holding the file.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::rename(wt.join(INPUTS_DIR), &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, wt.join(INPUTS_DIR)).unwrap();
        assert!(recover_stored(&wt, &p).is_err());
    }

    fn row_with(atts: serde_json::Value) -> Row {
        serde_json::from_value(serde_json::json!({"id": "a1", "status": "in_progress",
            "description": "d", "jobAttachments": atts}))
        .unwrap()
    }

    /// A declared size that is negative or above `file_bytes` refuses the
    /// generation; the bounds themselves pass.
    #[test]
    fn a_declared_size_outside_the_bounds_refuses_the_plan() {
        let file_bytes = 100;
        for (size, ok) in [(-1i64, false), (0, true), (100, true), (101, false)] {
            let row = row_with(serde_json::json!([{"id": "f1", "filename": "a.txt",
                "contentType": "text/plain", "byteSize": size, "sha256": "ab",
                "downloadUrl": "/f1"}]));
            let r = plan(&row, false, &[], file_bytes, None);
            assert_eq!(r.is_ok(), ok, "byteSize {size}: {r:?}");
        }
    }

    #[test]
    fn safe_names_keep_only_safe_characters() {
        assert_eq!(safe_name("../../.ssh/id_rsa"), "_.._.ssh_id_rsa");
        assert_eq!(safe_name("a b(1).pdf"), "a_b_1_.pdf");
        assert_eq!(safe_name("...hidden"), "hidden");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name("été.txt"), "_t_.txt");
        assert_eq!(safe_name(&"x".repeat(300)).len(), SAFE_NAME_MAX);
    }

    #[test]
    fn the_origin_check_refuses_foreign_hosts_userinfo_and_fragments() {
        let o = "https://market.near.ai";
        assert_eq!(
            check_origin(o, "/v1/jobs/j/attachments/a/raw").unwrap(),
            "https://market.near.ai/v1/jobs/j/attachments/a/raw"
        );
        assert!(check_origin(o, "https://market.near.ai/v1/files/x").is_ok());
        for bad in [
            "https://evil.example/x",
            "//evil.example/x",
            "http://market.near.ai/x",
            "https://market.near.ai:8443/x",
            "https://u:p@market.near.ai/x",
            "https://market.near.ai/x#frag",
            "relative/x",
        ] {
            assert!(check_origin(o, bad).is_err(), "{bad}");
        }
    }
}
