//! Agent A's tools (design 8 of the change `repo-audit-and-tiers`, spec
//! `repo-audit`): `nearbee audit fetch` takes a snapshot of the one public
//! GitHub repository the brief names, `nearbee audit finish` checks the
//! agent's candidate findings against it and renders the deliverable.
//!
//! Both run in the current directory, the run's worktree. The repository,
//! the brief and the candidates are untrusted data: nothing from them is
//! executed, links are never followed, every read and write is bounded.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The production GitHub API base.
pub const API_BASE: &str = "https://api.github.com";
/// The production archive base.
pub const CODELOAD_BASE: &str = "https://codeload.github.com";

/// The published snapshot directory in the worktree.
pub const SNAPSHOT_DIR: &str = "audit-snapshot";
/// The staging directory the snapshot is built in.
pub const STAGING_DIR: &str = "audit-snapshot.partial";

/// One request's limit.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// The whole fetch's limit, downloads and decoding included.
pub const FETCH_DEADLINE: Duration = Duration::from_secs(120);
/// The compressed archive cap.
pub const MAX_COMPRESSED: u64 = 100 * 1024 * 1024;
/// The decompressed cap, counted over every byte the decoder yields.
pub const MAX_DECOMPRESSED: u64 = 100 * 1024 * 1024;
/// The member cap.
pub const MAX_MEMBERS: usize = 20_000;
/// The largest file written.
pub const MAX_FILE: u64 = 1024 * 1024;
/// The prefix searched for a NUL byte.
pub const BINARY_PROBE: usize = 8 * 1024;

/// The largest input `finish` reads.
pub const MAX_INPUT: u64 = 2 * 1024 * 1024;
/// The most candidate findings accepted.
pub const MAX_FINDINGS: usize = 50;

/// The suffix agent instruction files get in the snapshot.
pub const REPO_DATA_SUFFIX: &str = ".repo-data";

/// File names written with [`REPO_DATA_SUFFIX`] at any depth.
const INSTRUCTION_FILES: [&str; 4] = ["agents.md", "claude.md", "gemini.md", "opencode.json"];
/// Directory names written with [`REPO_DATA_SUFFIX`] at any depth.
const INSTRUCTION_DIRS: [&str; 4] = [".claude", ".opencode", ".agents", ".cursor"];

/// The repository a snapshot was taken of (`source.json` without its
/// file list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub owner: String,
    pub repo: String,
    pub sha: String,
}

/// One written file of the snapshot, by its path under `repo/`, with the
/// hex SHA-256 of the bytes written: `finish` checks it before grounding,
/// so a file the agent step rewrote grounds nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// `audit-snapshot/source.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub version: u32,
    pub url: String,
    pub owner: String,
    pub repo: String,
    pub sha: String,
    pub files: Vec<SourceFile>,
}

/// `audit-snapshot/coverage.json`: written files and skips by reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub version: u32,
    pub files_written: u64,
    pub members: u64,
    /// Skipped members by reason: `link`, `special`, `unsafe_path`,
    /// `too_large`, `binary`.
    pub skipped: BTreeMap<String, u64>,
}

impl Coverage {
    fn skip(&mut self, reason: &str) {
        *self.skipped.entry(reason.to_owned()).or_insert(0) += 1;
    }

    /// Every skipped member, whatever the reason.
    pub fn skipped_total(&self) -> u64 {
        self.skipped.values().sum()
    }
}

/// What `fetch_snapshot` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// A new snapshot was published.
    Published { sha: String, coverage: Coverage },
    /// The published snapshot of the same repository was reused.
    Reused { sha: String },
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

/// `nearbee audit fetch` (design 8.3 to 8.5): scans the brief for exactly
/// one repository, reuses a published snapshot of it, else resolves the
/// public repository and its default branch's commit through
/// `api_base`, downloads the archive from `codeload_base` and publishes
/// the snapshot in `dir`. No redirect is followed; 20 s per request, 120 s
/// in total.
pub async fn fetch_snapshot(
    api_base: &str,
    codeload_base: &str,
    brief: &str,
    dir: &Path,
) -> Result<Fetched, String> {
    let started = Instant::now();
    let deadline = started + FETCH_DEADLINE;
    let repos = nearbee_core::intake::github_repos(brief);
    let [(owner, repo)] = repos.as_slice() else {
        return Err(format!(
            "{} ({} repositories named)",
            nearbee_core::intake::TEXT_GITHUB_REPO,
            repos.len()
        ));
    };
    if let Some(sha) = reusable_snapshot(dir, owner, repo) {
        return Ok(Fetched::Reused { sha });
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("nearbee-audit")
        .build()
        .map_err(|e| format!("the HTTP client cannot be built: {e}"))?;
    let api = api_base.trim_end_matches('/');
    let info = get_json(
        &client,
        &format!("{api}/repos/{owner}/{repo}"),
        deadline,
        "the repository",
    )
    .await?;
    if info.get("private").and_then(serde_json::Value::as_bool) != Some(false) {
        return Err("the repository is not public".to_owned());
    }
    let branch = info
        .get("default_branch")
        .and_then(serde_json::Value::as_str)
        .filter(|b| !b.is_empty())
        .ok_or_else(|| "the repository names no default branch".to_owned())?
        .to_owned();
    let commit = get_json(
        &client,
        &format!(
            "{api}/repos/{owner}/{repo}/commits/{}",
            encode_segment(&branch)
        ),
        deadline,
        "the default branch's commit",
    )
    .await?;
    let sha = commit
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| "the commit has no valid SHA".to_owned())?
        .to_ascii_lowercase();
    let archive = get_bytes(
        &client,
        &format!(
            "{}/{owner}/{repo}/tar.gz/{sha}",
            codeload_base.trim_end_matches('/')
        ),
        deadline,
        MAX_COMPRESSED,
    )
    .await?;
    let source = Source {
        url: format!("https://github.com/{owner}/{repo}"),
        owner: owner.clone(),
        repo: repo.clone(),
        sha: sha.clone(),
    };
    let dir = dir.to_path_buf();
    let coverage =
        tokio::task::spawn_blocking(move || build_snapshot(&dir, &archive, &source, deadline))
            .await
            .map_err(|e| format!("the archive decoding stopped: {e}"))??;
    Ok(Fetched::Published { sha, coverage })
}

/// The SHA of a published snapshot of `owner/repo` in `dir`, when its
/// `source.json` parses and names that repository (spec `repo-audit`,
/// Snapshot reuse). A link in place of the snapshot is never reused.
fn reusable_snapshot(dir: &Path, owner: &str, repo: &str) -> Option<String> {
    let meta = std::fs::symlink_metadata(dir.join(SNAPSHOT_DIR)).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let bytes = nearbee_core::manifest::read_beneath(
        dir,
        &format!("{SNAPSHOT_DIR}/source.json"),
        MAX_INPUT,
    )
    .ok()?;
    let s: SourceRecord = serde_json::from_slice(&bytes).ok()?;
    (s.version == 1 && s.owner.eq_ignore_ascii_case(owner) && s.repo.eq_ignore_ascii_case(repo))
        .then_some(s.sha)
}

/// A path segment with every byte outside `[A-Za-z0-9._-]` percent-encoded.
fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The time left before `deadline`, at most one request's limit.
fn request_limit(deadline: Instant) -> Result<Duration, String> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err("the fetch ran past its 120-second deadline".to_owned());
    }
    Ok(left.min(REQUEST_TIMEOUT))
}

async fn send(
    client: &reqwest::Client,
    url: &str,
    deadline: Instant,
    what: &str,
) -> Result<reqwest::Response, String> {
    let limit = request_limit(deadline)?;
    let resp = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .timeout(limit)
        .send()
        .await
        .map_err(|e| format!("{what}: the request failed: {}", e.without_url()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("{what}: status {}", status.as_u16()));
    }
    Ok(resp)
}

async fn get_json(
    client: &reqwest::Client,
    url: &str,
    deadline: Instant,
    what: &str,
) -> Result<serde_json::Value, String> {
    let bytes = read_capped(send(client, url, deadline, what).await?, MAX_INPUT, what).await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{what}: the answer is not JSON: {e}"))
}

async fn get_bytes(
    client: &reqwest::Client,
    url: &str,
    deadline: Instant,
    cap: u64,
) -> Result<Vec<u8>, String> {
    read_capped(
        send(client, url, deadline, "the archive").await?,
        cap,
        "the archive",
    )
    .await
}

async fn read_capped(mut resp: reqwest::Response, cap: u64, what: &str) -> Result<Vec<u8>, String> {
    if resp.content_length().is_some_and(|n| n > cap) {
        return Err(format!("{what}: larger than {cap} bytes"));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("{what}: the read failed: {}", e.without_url()))?
    {
        if out.len() as u64 + chunk.len() as u64 > cap {
            return Err(format!("{what}: larger than {cap} bytes"));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// A reader that fails once more than `cap` bytes went through it: every
/// byte the decoder yields counts, tar headers and skipped members
/// included (spec `repo-audit`, Archive limits).
struct Counting<R> {
    inner: R,
    read: u64,
    cap: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        if self.read > self.cap {
            return Err(std::io::Error::other(format!(
                "the archive decompresses past {} bytes",
                self.cap
            )));
        }
        Ok(n)
    }
}

/// Builds and publishes the snapshot of `source` from the gzip tar
/// `archive` in `dir` (design 8.5): a leftover staging directory (or a
/// link in its place) is removed without following it, a fresh one is
/// created, the members are written under `repo/`, `source.json` and
/// `coverage.json` are added, everything is synced, and staging is
/// renamed to `audit-snapshot/`. Any failure removes staging and
/// publishes nothing.
pub fn build_snapshot(
    dir: &Path,
    archive: &[u8],
    source: &Source,
    deadline: Instant,
) -> Result<Coverage, String> {
    let staging = dir.join(STAGING_DIR);
    remove_no_follow(&staging)?;
    if std::fs::symlink_metadata(dir.join(SNAPSHOT_DIR)).is_ok() {
        return Err(format!(
            "{SNAPSHOT_DIR}/ exists but is not a snapshot of {}/{}",
            source.owner, source.repo
        ));
    }
    std::fs::create_dir(&staging).map_err(|e| format!("{STAGING_DIR}: {e}"))?;
    let built = extract_into(&staging, archive, source, deadline).and_then(|coverage| {
        sync_tree(&staging)?;
        std::fs::rename(&staging, dir.join(SNAPSHOT_DIR))
            .map_err(|e| format!("publishing {SNAPSHOT_DIR}/: {e}"))?;
        sync_dir(dir)?;
        Ok(coverage)
    });
    if built.is_err() {
        let _ = remove_no_follow(&staging);
    }
    built
}

/// Removes `path` without following it: a link or file is unlinked, a
/// directory removed with its contents (links inside are unlinked, not
/// followed).
fn remove_no_follow(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
        Ok(m) if m.is_dir() => {
            std::fs::remove_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
        }
        Ok(_) => std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display())),
    }
}

fn extract_into(
    staging: &Path,
    archive: &[u8],
    source: &Source,
    deadline: Instant,
) -> Result<Coverage, String> {
    let repo_root = staging.join("repo");
    std::fs::create_dir(&repo_root).map_err(|e| format!("repo/: {e}"))?;
    let decoder = flate2::read::GzDecoder::new(archive);
    let counting = Counting {
        inner: decoder,
        read: 0,
        cap: MAX_DECOMPRESSED,
    };
    let mut tar = tar::Archive::new(counting);
    let mut coverage = Coverage {
        version: 1,
        ..Coverage::default()
    };
    let mut files: Vec<SourceFile> = Vec::new();
    let entries = tar
        .entries()
        .map_err(|e| format!("the archive cannot be read: {e}"))?;
    for entry in entries {
        if Instant::now() >= deadline {
            return Err("the fetch ran past its 120-second deadline".to_owned());
        }
        let mut entry = entry.map_err(|e| format!("the archive cannot be read: {e}"))?;
        coverage.members += 1;
        if coverage.members as usize > MAX_MEMBERS {
            return Err(format!("the archive has more than {MAX_MEMBERS} members"));
        }
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() || kind.is_dir() {
            continue;
        }
        if kind.is_symlink() || kind.is_hard_link() {
            coverage.skip("link");
            continue;
        }
        if !(kind.is_file() || kind.is_contiguous()) {
            coverage.skip("special");
            continue;
        }
        let raw = entry
            .path()
            .map_err(|e| format!("the archive cannot be read: {e}"))?
            .into_owned();
        let Some(rel) = member_path(&raw) else {
            coverage.skip("unsafe_path");
            continue;
        };
        if entry.size() > MAX_FILE {
            coverage.skip("too_large");
            continue;
        }
        let mut bytes = Vec::new();
        (&mut entry)
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("the archive cannot be read: {e}"))?;
        if bytes.len() as u64 > MAX_FILE {
            coverage.skip("too_large");
            continue;
        }
        if bytes[..bytes.len().min(BINARY_PROBE)].contains(&0) {
            coverage.skip("binary");
            continue;
        }
        let rel = rename_instructions(&rel);
        let target = repo_root.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("repo/{}: {e}", rel.display()))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    format!("repo/{}: a duplicate or colliding path", rel.display())
                } else {
                    format!("repo/{}: {e}", rel.display())
                }
            })?;
        file.write_all(&bytes)
            .map_err(|e| format!("repo/{}: {e}", rel.display()))?;
        files.push(SourceFile {
            path: path_text(&rel),
            size: bytes.len() as u64,
            sha256: nearbee_core::journal::sha256_hex(&bytes),
        });
        coverage.files_written += 1;
    }
    // The tar reader stops at the end-of-archive block; the rest of the
    // gzip stream (padding and the CRC and length trailer) is read to its
    // end, counted against the same cap, so a truncated or corrupt stream
    // fails (spec `repo-audit`, Archive limits).
    std::io::copy(&mut tar.into_inner(), &mut std::io::sink())
        .map_err(|e| format!("the archive cannot be read: {e}"))?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let record = SourceRecord {
        version: 1,
        url: source.url.clone(),
        owner: source.owner.clone(),
        repo: source.repo.clone(),
        sha: source.sha.clone(),
        files,
    };
    write_new(
        &staging.join("source.json"),
        &serde_json::to_vec_pretty(&record).expect("source.json serializes"),
    )?;
    write_new(
        &staging.join("coverage.json"),
        &serde_json::to_vec_pretty(&coverage).expect("coverage.json serializes"),
    )?;
    Ok(coverage)
}

/// The member's path without the archive's top directory, when every
/// component is a plain name; `None` for `..`, an absolute path, or the
/// top directory itself.
fn member_path(raw: &Path) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for c in raw.components() {
        match c {
            Component::Normal(n) => parts.push(n.to_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if parts.len() < 2 {
        return None;
    }
    Some(parts[1..].iter().collect())
}

/// Agent instruction and configuration files and directories, at any
/// depth, get [`REPO_DATA_SUFFIX`] (spec `repo-audit`, Repository
/// instructions are data); names compare case-insensitively.
fn rename_instructions(rel: &Path) -> PathBuf {
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let last = parts.len() - 1;
    let mut out = PathBuf::new();
    for (i, part) in parts.iter().enumerate() {
        let lower = part.to_ascii_lowercase();
        let rename = if i == last {
            INSTRUCTION_FILES.contains(&lower.as_str())
                || (lower == "copilot-instructions.md"
                    && i > 0
                    && parts[i - 1].eq_ignore_ascii_case(".github"))
        } else {
            INSTRUCTION_DIRS.contains(&lower.as_str())
        };
        if rename {
            out.push(format!("{part}{REPO_DATA_SUFFIX}"));
        } else {
            out.push(part);
        }
    }
    out
}

fn path_text(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(bytes)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn sync_dir(dir: &Path) -> Result<(), String> {
    std::fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("syncing {}: {e}", dir.display()))
}

/// Syncs every file and directory under `root` (which this process
/// created and filled; nothing in it is a link).
fn sync_tree(root: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", root.display()))?;
        let meta = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        if meta.is_dir() {
            sync_tree(&entry.path())?;
        } else if meta.is_file() {
            std::fs::File::open(entry.path())
                .and_then(|f| f.sync_all())
                .map_err(|e| format!("syncing {}: {e}", entry.path().display()))?;
        }
    }
    sync_dir(root)
}

// ---------------------------------------------------------------------------
// Finish
// ---------------------------------------------------------------------------

/// The candidates file the audit step writes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidates {
    version: u32,
    findings: Vec<Candidate>,
}

/// One candidate finding (spec `repo-audit`, Candidate findings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub severity: String,
    pub title: String,
    pub file: String,
    pub line_start: u64,
    pub line_end: u64,
    pub excerpt: String,
    pub detail: String,
    pub fix: String,
}

/// A rejected candidate with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejected {
    pub finding: Candidate,
    pub reason: String,
}

/// `out/findings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Findings {
    pub version: u32,
    pub repository: RepositoryRef,
    pub accepted: Vec<Candidate>,
    pub rejected: Vec<Rejected>,
    pub coverage: Coverage,
}

/// The audited repository and commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRef {
    pub url: String,
    pub sha: String,
}

/// The severities, most severe first.
pub const SEVERITIES: [&str; 5] = ["critical", "high", "medium", "low", "info"];

/// `nearbee audit finish` (design 8.6, spec `repo-audit`, Grounding check
/// and Deliverable): removes `out/result.json` first, checks every
/// candidate's location against the snapshot, writes `out/findings.json`
/// and `out/deliverable.md` through temporary names, and publishes
/// `out/result.json` last. Any failure leaves no `out/result.json`.
pub fn finish(dir: &Path) -> Result<Findings, String> {
    let out = dir.join("out");
    match std::fs::symlink_metadata(&out) {
        Ok(m) if !m.is_dir() => return Err("out/ is not a directory".to_owned()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&out).map_err(|e| format!("out/: {e}"))?;
        }
        Err(e) => return Err(format!("out/: {e}")),
    }
    remove_no_follow(&out.join("result.json"))?;
    let read = |rel: &str| nearbee_core::manifest::read_beneath(dir, rel, MAX_INPUT);
    let candidates: Candidates = serde_json::from_slice(&read("audit/candidates.json")?)
        .map_err(|e| format!("audit/candidates.json is invalid: {e}"))?;
    if candidates.version != 1 {
        return Err(format!(
            "audit/candidates.json has version {}; only version 1 is read",
            candidates.version
        ));
    }
    if candidates.findings.len() > MAX_FINDINGS {
        return Err(format!(
            "audit/candidates.json has {} findings; at most {MAX_FINDINGS} are read",
            candidates.findings.len()
        ));
    }
    let source: SourceRecord =
        serde_json::from_slice(&read(&format!("{SNAPSHOT_DIR}/source.json"))?)
            .map_err(|e| format!("{SNAPSHOT_DIR}/source.json is invalid: {e}"))?;
    let coverage: Coverage =
        serde_json::from_slice(&read(&format!("{SNAPSHOT_DIR}/coverage.json"))?)
            .map_err(|e| format!("{SNAPSHOT_DIR}/coverage.json is invalid: {e}"))?;
    let known: BTreeMap<&str, &str> = source
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for c in candidates.findings {
        match ground(dir, &known, &c) {
            Ok(()) => accepted.push(c),
            Err(reason) => rejected.push(Rejected { finding: c, reason }),
        }
    }
    accepted.sort_by_key(|c| severity_rank(&c.severity));
    let findings = Findings {
        version: 1,
        repository: RepositoryRef {
            url: source.url.clone(),
            sha: source.sha.clone(),
        },
        accepted,
        rejected,
        coverage,
    };
    let json = serde_json::to_string_pretty(&findings).expect("findings serialize");
    let markdown = render(&source, &findings, &json);
    write_replacing(&out, "findings.json", json.as_bytes())?;
    write_replacing(&out, "deliverable.md", markdown.as_bytes())?;
    let manifest = nearbee_core::manifest::Manifest {
        version: 1,
        markdown: nearbee_core::manifest::Entry {
            path: "deliverable.md".into(),
            size: markdown.len() as u64,
            sha256: nearbee_core::journal::sha256_hex(markdown.as_bytes()),
        },
        files: Vec::new(),
        evidence: Vec::new(),
    };
    write_replacing(
        &out,
        "result.json",
        &serde_json::to_vec_pretty(&manifest).expect("the manifest serializes"),
    )?;
    Ok(findings)
}

fn severity_rank(s: &str) -> usize {
    SEVERITIES
        .iter()
        .position(|x| *x == s)
        .unwrap_or(SEVERITIES.len())
}

/// Writes `name` in `out` through a temporary name and a rename, synced.
fn write_replacing(out: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let tmp = out.join(format!(".{name}.tmp"));
    remove_no_follow(&tmp)?;
    write_new(&tmp, bytes)?;
    std::fs::File::open(&tmp)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("out/{name}: {e}"))?;
    std::fs::rename(&tmp, out.join(name)).map_err(|e| format!("out/{name}: {e}"))?;
    sync_dir(out)
}

/// Lines trimmed, blank lines dropped.
fn normalized(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// The location check of one candidate: its fields are in range, its file
/// is in the snapshot's list and still holds the bytes fetch wrote (the
/// recorded SHA-256), its lines exist and its excerpt is a contiguous run
/// of the cited lines (each trimmed, blanks dropped). A passing finding
/// is "location checked", not proved.
fn ground(dir: &Path, known: &BTreeMap<&str, &str>, c: &Candidate) -> Result<(), String> {
    let chars = |s: &str| s.chars().count();
    if !SEVERITIES.contains(&c.severity.as_str()) {
        return Err("unknown severity".to_owned());
    }
    if !(1..=120).contains(&chars(&c.title)) {
        return Err("title length out of range".to_owned());
    }
    if !(1..=1500).contains(&chars(&c.detail)) {
        return Err("detail length out of range".to_owned());
    }
    if !(1..=1000).contains(&chars(&c.fix)) {
        return Err("fix length out of range".to_owned());
    }
    if chars(&c.excerpt) > 400 {
        return Err("excerpt longer than 400 characters".to_owned());
    }
    let Some(recorded) = known.get(c.file.as_str()) else {
        return Err("unknown file".to_owned());
    };
    if c.line_start == 0 || c.line_end < c.line_start {
        return Err("line out of range".to_owned());
    }
    let excerpt = normalized(&c.excerpt);
    if excerpt.is_empty() {
        return Err("empty excerpt".to_owned());
    }
    let bytes = nearbee_core::manifest::read_beneath(
        dir,
        &format!("{SNAPSHOT_DIR}/repo/{}", c.file),
        MAX_INPUT,
    )
    .map_err(|_| "unknown file".to_owned())?;
    if nearbee_core::journal::sha256_hex(&bytes) != *recorded {
        return Err("file changed after fetch".to_owned());
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    if c.line_end > lines.len() as u64 {
        return Err("line out of range".to_owned());
    }
    let cited = normalized_lines(&lines[(c.line_start - 1) as usize..c.line_end as usize]);
    if cited
        .windows(excerpt.len())
        .any(|w| w == excerpt.as_slice())
    {
        Ok(())
    } else {
        Err("excerpt not contiguous in the cited lines".to_owned())
    }
}

fn normalized_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Escapes untrusted text for Markdown (design 8.7): `\`, `` ` ``, `*`,
/// `_`, `[`, `]`, `#`, `|` by backslash, `<`, `>`, `&` as entities, and a
/// line start of `-` or `+` by a backslash before it, of `<digits>.` by a
/// backslash before the dot, of a run of three or more `~` (a tilde code
/// fence) by a backslash before the first tilde.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        out.push_str(indent);
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        // An ordered list marker is escaped at its dot (`12\.`), a bullet
        // at its sign.
        let ordered = digits > 0 && trimmed[digits..].starts_with('.');
        if trimmed.starts_with('-') || trimmed.starts_with('+') || trimmed.starts_with("~~~") {
            out.push('\\');
        }
        for (pos, ch) in trimmed.char_indices() {
            if ordered && pos == digits {
                out.push_str("\\.");
                continue;
            }
            match ch {
                '\\' | '`' | '*' | '_' | '[' | ']' | '#' | '|' => {
                    out.push('\\');
                    out.push(ch);
                }
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '&' => out.push_str("&amp;"),
                '\r' => {}
                _ => out.push(ch),
            }
        }
    }
    out
}

/// A code fence long enough for `text`: `max(3, longest backtick run + 1)`
/// backticks.
pub fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

/// The deliverable Markdown (spec `repo-audit`, Deliverable).
fn render(source: &SourceRecord, f: &Findings, json: &str) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "# Repository audit: {}/{}\n\n",
        escape(&source.owner),
        escape(&source.repo)
    ));
    md.push_str(&format!(
        "Repository {} at commit {}.\n\n",
        escape(&source.url),
        escape(&source.sha)
    ));
    md.push_str(&format!(
        "{} findings are location checked, {} candidate findings were rejected, {} files were skipped.\n\n",
        f.accepted.len(),
        f.rejected.len(),
        f.coverage.skipped_total()
    ));
    md.push_str(
        "Location checked means the cited file, lines and excerpt exist in the snapshot; it does not prove that an issue is real.\n",
    );
    let mut n = 0;
    for sev in SEVERITIES {
        let group: Vec<&Candidate> = f.accepted.iter().filter(|c| c.severity == sev).collect();
        if group.is_empty() {
            continue;
        }
        let mut title = sev.to_owned();
        title[..1].make_ascii_uppercase();
        md.push_str(&format!("\n## {title}\n"));
        for c in group {
            n += 1;
            md.push_str(&format!("\n### {n}. {}\n\n", escape(&c.title)));
            md.push_str(&format!(
                "File {}, lines {} to {}.\n\n",
                escape(&c.file),
                c.line_start,
                c.line_end
            ));
            for line in c.excerpt.lines() {
                md.push_str(&format!("> {}\n", escape(line)));
            }
            md.push_str(&format!(
                "\n{}\n\nFix: {}\n",
                escape(&c.detail),
                escape(&c.fix)
            ));
        }
    }
    if f.accepted.is_empty() {
        md.push_str("\nNo finding passed the location check.\n");
    }
    md.push_str(&format!(
        "\n## Not reported\n\n{} candidate findings were rejected by the location check. {} archive members were skipped",
        f.rejected.len(),
        f.coverage.skipped_total()
    ));
    if f.coverage.skipped.is_empty() {
        md.push_str(".\n");
    } else {
        let parts: Vec<String> = f
            .coverage
            .skipped
            .iter()
            .map(|(k, v)| format!("{} {v}", k.replace('_', " ")))
            .collect();
        md.push_str(&format!(": {}.\n", parts.join(", ")));
    }
    let fence = fence_for(json);
    md.push_str(&format!(
        "\n## Findings JSON\n\n{fence}json\n{json}\n{fence}\n"
    ));
    md
}
