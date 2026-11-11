//! What Nearbee keeps per hire, how the operator sees it (`nearbee
//! hires`) and how it is deleted (`nearbee cleanup`), spec
//! `hire-retention`, design 14.
//!
//! Everything here reads the hire journals under `<state_dir>/hires/`; a
//! journal that cannot be read is reported on its own line and never hides
//! the others. Summaries carry no brief text, token or capability link.

use std::path::{Path, PathBuf};

use serde::Serialize;
use time::OffsetDateTime;

use crate::cost;
use crate::journal::{self, Event, JournalRead, Record};

/// One hire as `nearbee hires` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// The hire id (the journal directory name when the journal is broken).
    pub hire_id: String,
    /// The agent handle, when the journal recorded it.
    pub handle: Option<String>,
    /// `open generation <n>`, `delivered`, `frozen`, `closed accepted`,
    /// `closed cancelled`, `closed dispute resolved`, `deleted`, or
    /// `unreadable`.
    pub state: String,
    pub generation: u32,
    pub latest_run: Option<String>,
    /// USD, a decimal string.
    pub spent_usd: String,
    pub deadline: Option<String>,
    /// The operator attention code, when one is set.
    pub attention: Option<String>,
    /// Why the journal cannot be read.
    pub problem: Option<String>,
}

/// One hire's journal as read from disk.
pub struct Entry {
    /// The journal directory (`<state_dir>/hires/<safe>`).
    pub dir: PathBuf,
    pub read: Result<JournalRead, String>,
}

/// Every hire journal under the state directory, sorted by directory
/// name, and why the listing is incomplete when it is.
pub struct Scan {
    pub entries: Vec<Entry>,
    /// Why `hires/` (or one of its entries) could not be listed: the scan
    /// then misses journals, and whatever it would decide about the slot
    /// or the daily loss is unknown (review round 3, R3-1). A missing
    /// `hires/` is complete and empty.
    pub problem: Option<String>,
}

/// Lists every hire journal under the state directory.
pub fn scan(state_dir: &Path) -> Scan {
    let mut out = Vec::new();
    let dirs = match std::fs::read_dir(state_dir.join("hires")) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Scan {
                entries: out,
                problem: None,
            };
        }
        Err(e) => {
            return Scan {
                entries: out,
                problem: Some(format!(
                    "the hires directory cannot be listed ({})",
                    e.kind()
                )),
            };
        }
    };
    let mut problem = None;
    for d in dirs {
        let d = match d {
            Ok(d) => d,
            Err(e) => {
                problem = Some(format!(
                    "an entry of the hires directory cannot be listed ({})",
                    e.kind()
                ));
                continue;
            }
        };
        let dir = d.path();
        if !dir.is_dir() {
            continue;
        }
        let path = dir.join("journal.jsonl");
        let read = match std::fs::read_to_string(&path) {
            Ok(text) => match journal::read(&text) {
                Ok(j) if j.truncated_line.is_some() => Err(format!(
                    "the journal is truncated at line {}",
                    j.truncated_line.unwrap_or(0)
                )),
                Ok(j) => Ok(j),
                Err(_) => Err("the journal does not parse".into()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => Err(format!("the journal cannot be read ({})", e.kind())),
        };
        out.push(Entry { dir, read });
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    Scan {
        entries: out,
        problem,
    }
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn generation_of(j: &JournalRead) -> u32 {
    j.records
        .iter()
        .filter_map(|r| match &r.event {
            Event::GenerationOpened(g) => Some(g.generation),
            _ => None,
        })
        .max()
        .unwrap_or(1)
}

fn last_lifecycle(records: &[Record]) -> Option<(&journal::Lifecycle, OffsetDateTime)> {
    records.iter().rev().find_map(|r| match &r.event {
        Event::Lifecycle(l) => Some((l, r.timestamp)),
        _ => None,
    })
}

fn delivered_any(records: &[Record]) -> bool {
    records
        .iter()
        .any(|r| matches!(&r.event, Event::Decision(d) if d.decision == "delivered"))
}

/// The decision closing a generation, with its time.
fn gen_decision(
    records: &[Record],
    generation: u32,
) -> Option<(&journal::Decision, OffsetDateTime)> {
    records
        .iter()
        .rev()
        .filter(|r| r.generation == generation)
        .find_map(|r| match &r.event {
            Event::Decision(d) => Some((d, r.timestamp)),
            _ => None,
        })
}

/// The last `deleted` record and whether it removed everything it named.
fn last_deleted(records: &[Record]) -> Option<bool> {
    records.iter().rev().find_map(|r| match &r.event {
        // A refused path is never touched again, so only a failure keeps
        // the hire eligible for the next cleanup.
        Event::Deleted(d) => Some(d.what.iter().all(|w| !w.starts_with("failed "))),
        _ => None,
    })
}

/// How a hire stands, for the listing and the eligibility.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Standing {
    Open(u32),
    Delivered,
    Frozen,
    Accepted { at: OffsetDateTime },
    Cancelled { delivered: bool, at: OffsetDateTime },
    DisputeResolved { at: OffsetDateTime },
}

fn standing(j: &JournalRead) -> Standing {
    let generation = generation_of(j);
    if let Some((l, at)) = last_lifecycle(&j.records) {
        match l.status.as_str() {
            "accepted" => {
                let at = l
                    .finalized_at
                    .as_deref()
                    .and_then(|f| {
                        OffsetDateTime::parse(f, &time::format_description::well_known::Rfc3339)
                            .ok()
                    })
                    .unwrap_or(at);
                return Standing::Accepted { at };
            }
            "cancelled" => {
                return Standing::Cancelled {
                    delivered: delivered_any(&j.records),
                    at,
                };
            }
            "disputed" => return Standing::Frozen,
            "dispute_resolved" => return Standing::DisputeResolved { at },
            _ => {}
        }
    }
    match gen_decision(&j.records, generation) {
        Some((d, _)) if d.decision == "delivered" => Standing::Delivered,
        // A decline that landed closes the row (`cancelled` with the
        // decline's reason), recorded or not.
        Some((d, at)) if d.decision == "decline" => Standing::Cancelled {
            delivered: delivered_any(&j.records),
            at,
        },
        _ => Standing::Open(generation),
    }
}

fn state_text(s: &Standing) -> String {
    match s {
        Standing::Open(n) => format!("open generation {n}"),
        Standing::Delivered => "delivered".into(),
        Standing::Frozen => "frozen".into(),
        Standing::Accepted { .. } => "closed accepted".into(),
        Standing::Cancelled { .. } => "closed cancelled".into(),
        Standing::DisputeResolved { .. } => "closed dispute resolved".into(),
    }
}

/// The summary of one journal.
pub fn summary(e: &Entry) -> Summary {
    let j = match &e.read {
        Ok(j) => j,
        Err(problem) => {
            return Summary {
                hire_id: dir_name(&e.dir),
                handle: None,
                state: "unreadable".into(),
                generation: 0,
                latest_run: None,
                spent_usd: "0".into(),
                deadline: None,
                attention: None,
                problem: Some(problem.clone()),
            };
        }
    };
    let hire_id = j
        .records
        .first()
        .map(|r| r.hire_id.clone())
        .unwrap_or_else(|| dir_name(&e.dir));
    let handle = j.records.iter().find_map(|r| match &r.event {
        Event::Received(rc) => rc.listing_id.clone(),
        _ => None,
    });
    let latest_run = j.records.iter().rev().find_map(|r| match &r.event {
        Event::RunObserved(o) => Some(o.run_id.clone()),
        _ => None,
    });
    let deadline = j.records.iter().rev().find_map(|r| match &r.event {
        Event::Intent(i) if i.kind == "run_start" => i
            .refs
            .get("sla_deadline_at")
            .filter(|d| !d.is_empty())
            .cloned(),
        _ => None,
    });
    let attention = j
        .records
        .iter()
        .rev()
        .find_map(|r| match &r.event {
            Event::Attention(a) => Some(a.code.clone()),
            _ => None,
        })
        .filter(|c| c != "none");
    let state = if last_deleted(&j.records) == Some(true) {
        "deleted".to_owned()
    } else {
        state_text(&standing(j))
    };
    Summary {
        hire_id,
        handle,
        state,
        generation: generation_of(j),
        latest_run,
        spent_usd: cost::spent(&j.records).normalize().to_string(),
        deadline,
        attention,
        problem: None,
    }
}

/// Every hire's summary.
pub fn summaries(state_dir: &Path) -> Vec<Summary> {
    let scan = scan(state_dir);
    let mut out: Vec<Summary> = scan.entries.iter().map(summary).collect();
    if let Some(problem) = scan.problem {
        out.push(Summary {
            hire_id: "hires".into(),
            handle: None,
            state: "unreadable".into(),
            generation: 0,
            latest_run: None,
            spent_usd: "0".into(),
            deadline: None,
            attention: None,
            problem: Some(problem),
        });
    }
    out
}

/// What cleanup would do for one hire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub hire_id: String,
    pub eligible: bool,
    /// Why the hire is or is not eligible.
    pub why: String,
    /// Paths Nearbee owns and deletes.
    pub delete: Vec<PathBuf>,
    /// Paths a journal names outside the workspace's `work/`: never
    /// touched.
    pub refused: Vec<PathBuf>,
    /// What Nearbee cannot delete, by name.
    pub not_covered: Vec<String>,
    /// Owned data the plan could not enumerate (a hire directory whose
    /// `gen-*` snapshots cannot be listed, R4-5): each is a failure line
    /// of the cleanup, so the hire stays eligible until it is removed.
    pub unlisted: Vec<String>,
    #[serde(skip)]
    journal_dir: PathBuf,
}

/// The hire's worktrees: every journaled one, else the per-hire default.
fn worktrees(j: &JournalRead, workspace: &Path, hire_id: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = j
        .records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Intent(i) if i.kind == "run_start" => i.refs.get("worktree").map(PathBuf::from),
            _ => None,
        })
        .collect();
    out.push(workspace.join("work").join(journal::safe_id(hire_id)));
    out.sort();
    out.dedup();
    out
}

/// Whether `p` lies inside `<workspace>/work/` after canonicalisation (a
/// missing path is judged by its nearest existing ancestor).
fn inside_work(p: &Path, workspace: &Path) -> bool {
    let Ok(work) = workspace.join("work").canonicalize() else {
        return false;
    };
    let mut probe = p.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(c) = probe.canonicalize() {
            let mut full = c;
            for part in rest.iter().rev() {
                full.push(part);
            }
            return full.starts_with(&work) && full != work;
        }
        match (probe.file_name(), probe.parent()) {
            (Some(name), Some(parent)) => {
                if name == ".." {
                    return false;
                }
                rest.push(name.to_owned());
                probe = parent.to_path_buf();
            }
            _ => return false,
        }
    }
}

/// The snapshot directories of a hire (`gen-*`). A directory or entry
/// that cannot be listed is an error, never an empty list (R4-5): the
/// snapshots it hides are buyer data a cleanup must not claim deleted.
fn snapshot_dirs(journal_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(journal_dir)? {
        let e = e?;
        if e.file_name().to_string_lossy().starts_with("gen-") && e.file_type()?.is_dir() {
            out.push(e.path());
        }
    }
    out.sort();
    Ok(out)
}

fn eligibility(j: &JournalRead, days: u32, now: OffsetDateTime) -> (bool, String) {
    if !j.open_intents.is_empty() {
        return (false, "an intent is open".into());
    }
    // The same predicate that holds the apb slot: an adopted run still
    // running, an open abort or an unresolved start keeps the hire's data
    // in use, whatever its market side says (R3-3).
    if crate::serve::local_work_open(j) {
        return (
            false,
            "the hire's local work is not settled (a run may still be active)".into(),
        );
    }
    if last_deleted(&j.records) == Some(true) {
        return (false, "already deleted".into());
    }
    let keep = time::Duration::days(i64::from(days));
    match standing(j) {
        Standing::Open(_) | Standing::Delivered => (false, "the hire is not closed".into()),
        Standing::Frozen => (false, "the hire is disputed (frozen)".into()),
        Standing::Accepted { at } if now - at >= keep => {
            (true, format!("accepted more than {days} days ago"))
        }
        Standing::Accepted { .. } => (false, format!("accepted less than {days} days ago")),
        Standing::Cancelled {
            delivered: false, ..
        } => (true, "closed without a delivery".into()),
        Standing::Cancelled { at, .. } | Standing::DisputeResolved { at } if now - at >= keep => (
            true,
            format!("closed after a delivery more than {days} days ago"),
        ),
        Standing::Cancelled { .. } | Standing::DisputeResolved { .. } => (
            false,
            format!("closed after a delivery less than {days} days ago"),
        ),
    }
}

/// The cleanup plan of every hire (`--dry-run` prints it as is).
pub fn plan(
    state_dir: &Path,
    workspace: &Path,
    retention_days: u32,
    now: OffsetDateTime,
) -> Vec<Plan> {
    let mut out = Vec::new();
    let scan = scan(state_dir);
    if let Some(problem) = &scan.problem {
        out.push(Plan {
            hire_id: "hires".into(),
            eligible: false,
            why: problem.clone(),
            delete: Vec::new(),
            refused: Vec::new(),
            not_covered: Vec::new(),
            unlisted: Vec::new(),
            journal_dir: state_dir.join("hires"),
        });
    }
    for e in scan.entries {
        let j = match &e.read {
            Ok(j) => j,
            Err(problem) => {
                out.push(Plan {
                    hire_id: dir_name(&e.dir),
                    eligible: false,
                    why: problem.clone(),
                    delete: Vec::new(),
                    refused: Vec::new(),
                    not_covered: Vec::new(),
                    unlisted: Vec::new(),
                    journal_dir: e.dir.clone(),
                });
                continue;
            }
        };
        let hire_id = j
            .records
            .first()
            .map(|r| r.hire_id.clone())
            .unwrap_or_else(|| dir_name(&e.dir));
        let (eligible, why) = eligibility(j, retention_days, now);
        let mut delete = Vec::new();
        let mut refused = Vec::new();
        for wt in worktrees(j, workspace, &hire_id) {
            if inside_work(&wt, workspace) {
                delete.push(wt);
            } else {
                refused.push(wt);
            }
        }
        let mut unlisted = Vec::new();
        match snapshot_dirs(&e.dir) {
            Ok(dirs) => delete.extend(dirs),
            Err(err) => unlisted.push(format!(
                "failed {}: the snapshot directories cannot be listed ({})",
                e.dir.display(),
                err.kind()
            )),
        }
        // A path under another listed one goes with it.
        let all = delete.clone();
        delete.retain(|p| !all.iter().any(|q| q != p && p.starts_with(q)));
        let runs: Vec<String> = {
            let mut r: Vec<String> = j
                .records
                .iter()
                .filter_map(|r| match &r.event {
                    Event::RunObserved(o) => Some(o.run_id.clone()),
                    _ => None,
                })
                .collect();
            r.sort();
            r.dedup();
            r
        };
        let mut not_covered = Vec::new();
        if !runs.is_empty() {
            not_covered.push(format!("apb run directories of {}", runs.join(", ")));
        }
        not_covered.push("apb .apb/decisions.jsonl entries of the scope check".into());
        not_covered.push("executor caches".into());
        out.push(Plan {
            hire_id,
            eligible,
            why,
            delete,
            refused,
            not_covered,
            unlisted,
            journal_dir: e.dir.clone(),
        });
    }
    out
}

/// Deletes one eligible hire's paths and journals the inventory in a
/// `deleted` record. Returns the inventory lines.
pub fn execute(p: &Plan) -> Result<Vec<String>, journal::WriterError> {
    let mut what = Vec::new();
    for path in &p.delete {
        // The removal is durable before the record says so: the parent
        // directory is synced, and a failed sync keeps the hire eligible
        // for the next cleanup (R3-7). An absent path passes the same
        // barrier (R4-4): an earlier removal whose sync failed is only
        // settled once its directory is synced.
        let line = match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match sync_parent(path) {
                Ok(()) => format!("absent {}", path.display()),
                Err(e) => format!("failed {}: {}", path.display(), e.kind()),
            },
            _ => match std::fs::remove_dir_all(path).and_then(|()| sync_parent(path)) {
                Ok(()) => format!("removed {}", path.display()),
                Err(e) => format!("failed {}: {}", path.display(), e.kind()),
            },
        };
        what.push(line);
    }
    what.extend(p.unlisted.iter().cloned());
    for path in &p.refused {
        what.push(format!(
            "refused {} (outside the workspace's work directory)",
            path.display()
        ));
    }
    for n in &p.not_covered {
        what.push(format!("not covered: {n}"));
    }
    let journal_path = p.journal_dir.join("journal.jsonl");
    let mut w = journal::Writer::open(journal_path, &p.hire_id, crate::clock::Clock::real())?;
    w.append(Event::Deleted(journal::Deleted {
        what: what.clone(),
        reason: "cleanup".into(),
    }))?;
    Ok(what)
}

/// Syncs the directory that held `path`, so its removal survives a power
/// loss. When that directory is gone too, its nearest existing ancestor
/// is synced: that is where the topmost removed entry was.
fn sync_parent(path: &Path) -> std::io::Result<()> {
    let mut dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the path has no parent"))?;
    loop {
        match std::fs::File::open(dir) {
            Ok(f) => return f.sync_all(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                dir = dir
                    .parent()
                    .ok_or_else(|| std::io::Error::other("no ancestor exists"))?;
            }
            Err(e) => return Err(e),
        }
    }
}
