//! Hire journal record types, layout and a pure reader.
//!
//! One journal per hire at `<state_dir>/hires/<safe-id>/journal.jsonl`, one
//! JSON record per line. Stage S1 defines the format only; writing, locking
//! and recovery arrive in S3. Records never hold the buyer's brief text,
//! attachment contents, secrets or model transcripts: digests, sizes, ids and
//! file names only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

/// The newest record schema this version reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Characters of the readable prefix kept by [`safe_id`].
pub const SAFE_ID_PREFIX: usize = 64;
/// Hex digits of the id's SHA-256 appended by [`safe_id`].
pub const SAFE_ID_DIGEST_HEX: usize = 16;

/// A filesystem-safe, stable directory name for a market hire id: characters
/// outside `[A-Za-z0-9_-]` become `_`, the result is cut to
/// [`SAFE_ID_PREFIX`] characters, then `-` and the first
/// [`SAFE_ID_DIGEST_HEX`] hex digits of the id's SHA-256 are appended, so
/// ids that sanitize or cut alike stay distinct and the name stays far below
/// the 255-byte file name limit.
pub fn safe_id(hire_id: &str) -> String {
    let clean: String = hire_id
        .chars()
        .take(SAFE_ID_PREFIX)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let digest = hex::encode(Sha256::digest(hire_id.as_bytes()));
    format!("{clean}-{}", &digest[..SAFE_ID_DIGEST_HEX])
}

/// `<state_dir>/hires/<safe-id>/journal.jsonl`.
pub fn journal_path(state_dir: &Path, hire_id: &str) -> PathBuf {
    state_dir
        .join("hires")
        .join(safe_id(hire_id))
        .join("journal.jsonl")
}

/// SHA-256 of bytes as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// One journal line.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub schema_version: u32,
    pub seq: u64,
    pub timestamp: OffsetDateTime,
    pub hire_id: String,
    /// The hire generation this record belongs to; every record written
    /// from this version carries it, a record without one (S2) reads as 1.
    pub generation: u32,
    pub event: Event,
}

/// The typed payload of a record. `Unknown` keeps records of event types a
/// newer version wrote, so reading never fails on them.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Received(Received),
    Decision(Decision),
    Intent(Intent),
    Result(OpResult),
    RunObserved(RunObserved),
    ArtifactInventory(ArtifactInventory),
    Deleted(Deleted),
    Intake(Intake),
    GenerationOpened(GenerationOpened),
    Lifecycle(Lifecycle),
    Inputs(Inputs),
    Attention(Attention),
    EvidenceCollected(EvidenceCollected),
    EvidenceFailed(EvidenceFailed),
    PrivacyFlagMissing(PrivacyFlagMissing),
    Unknown {
        event_type: String,
        raw: serde_json::Value,
    },
}

impl Event {
    pub fn event_type(&self) -> &str {
        match self {
            Event::Received(_) => "received",
            Event::Decision(_) => "decision",
            Event::Intent(_) => "intent",
            Event::Result(_) => "result",
            Event::RunObserved(_) => "run_observed",
            Event::ArtifactInventory(_) => "artifact_inventory",
            Event::Deleted(_) => "deleted",
            Event::Intake(_) => "intake",
            Event::GenerationOpened(_) => "generation_opened",
            Event::Lifecycle(_) => "lifecycle",
            Event::Inputs(_) => "inputs",
            Event::Attention(_) => "attention",
            Event::EvidenceCollected(_) => "evidence_collected",
            Event::EvidenceFailed(_) => "evidence_failed",
            Event::PrivacyFlagMissing(_) => "privacy_flag_missing",
            Event::Unknown { event_type, .. } => event_type,
        }
    }
}

/// A hire arrived. Holds the brief's digest and length, never its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Received {
    pub brief_sha256: String,
    pub brief_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<FileRef>,
}

impl Received {
    /// Builds the record payload from a brief, keeping only digest and length.
    pub fn from_brief(brief: &str) -> Self {
        Self {
            brief_sha256: sha256_hex(brief.as_bytes()),
            brief_bytes: brief.len() as u64,
            listing_id: None,
            attachments: Vec::new(),
        }
    }
}

/// A file named by path, size and digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// A decision Nearbee took about the hire (accept, decline, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub decision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// An external write about to be made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    /// Stable id of the operation (UUID v7), repeated by its result.
    pub operation_id: Uuid,
    /// Operation kind, e.g. `run_start`, `market_submit`.
    pub kind: String,
    /// Execution number within the hire (1 for the first run).
    pub execution: u32,
    /// Digest of the request, to tell a repeat from a different request.
    pub request_digest: String,
    /// References needed to repeat or look up the operation.
    #[serde(default)]
    pub refs: BTreeMap<String, String>,
}

/// The outcome of an intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpResult {
    pub operation_id: Uuid,
    pub outcome: String,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

/// A run's state as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunObserved {
    pub run_id: String,
    pub execution: u32,
    pub status: String,
    /// The playbook version the run reports (round 5, codex 4). `None`
    /// when the engine named none, and on the records written at a known
    /// start before the first poll; serve never trusts the output of a
    /// finished run whose version is `None` or not the published one.
    #[serde(default)]
    pub playbook_version: Option<String>,
    /// The run's total cost at its terminal observation, as a decimal
    /// string; `None` when the engine named none or the run is not
    /// terminal. The end and its cost land in one record (design 8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<String>,
    /// Whether the reported cost covers every finished attempt (design 8);
    /// `None` when the engine named none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_complete: Option<bool>,
    /// The per-run budget the run started with, as a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<String>,
    /// The per-attempt model records of the terminal observation
    /// (design 7). A record written without the field (S3) reads as
    /// empty; non-terminal observations carry none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<crate::runner::AttemptModel>,
}

/// The files a run produced, by name, size and digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactInventory {
    pub files: Vec<FileRef>,
}

/// What was deleted and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deleted {
    pub what: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// An intake check's outcome (design 2). The payload never names the
/// brief, only the check, the outcome and the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intake {
    pub check: String,
    /// `passed`, `declined` or `skipped`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// Whether the decline fell within one hour of the row's `createdAt`.
    #[serde(default)]
    pub within_first_hour: bool,
}

/// A reopened row first seen: a generation of kind `rework` or `redo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationOpened {
    pub generation: u32,
    pub kind: String,
    /// SHA-256 of the row's `slaDeadlineAt`, `deliverableUrl` and
    /// `submittedAt` (the generation key).
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_bytes: Option<u64>,
}

/// A new row status seen for the first time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lifecycle {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<String>,
}

/// The verified files of a generation, by attachment id and path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inputs {
    pub files: Vec<InputFile>,
}

/// One verified input file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFile {
    pub attachment_id: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// The hire needs the operator; cleared by a later `attention` with
/// `code: none`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attention {
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// A market run whose detail did not report the apb external-data flag
/// (S6 design 3.6): the run was aborted and the hire declined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyFlagMissing {
    pub run_id: String,
}

/// An endpoint attestation report stored for the generation (design
/// 6.2): its phase (`start` or `end`), the model and the SHA-256 of the
/// report body. The body itself lives in the generation's private
/// snapshot file, never in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCollected {
    pub phase: String,
    pub model: String,
    pub sha256: String,
}

/// A failed report fetch (design 6.2): phase, model and a short redacted
/// reason of at most 200 characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceFailed {
    pub phase: String,
    pub model: String,
    pub reason: String,
}

/// The wire envelope. `generation` is additive: a record written without
/// it (S2) reads as generation 1.
#[derive(Serialize, Deserialize)]
struct Wire {
    schema_version: u32,
    seq: u64,
    #[serde(with = "time::serde::rfc3339")]
    timestamp: OffsetDateTime,
    hire_id: String,
    #[serde(default = "default_generation")]
    generation: u32,
    event_type: String,
    payload: serde_json::Value,
}

fn default_generation() -> u32 {
    1
}

/// A journal read failure.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum JournalError {
    #[error("journal line {line}: schema_version {found} is newer than supported {SCHEMA_VERSION}")]
    UnsupportedVersion { line: usize, found: u32 },
    #[error("journal line {line}: {message}")]
    Malformed { line: usize, message: String },
    #[error("journal line {line}: seq {found} does not follow {previous}")]
    Sequence {
        line: usize,
        previous: u64,
        found: u64,
    },
}

impl Record {
    /// Serializes the record as one JSON line (without the newline).
    pub fn to_line(&self) -> String {
        let payload = match &self.event {
            Event::Received(p) => to_value(p),
            Event::Decision(p) => to_value(p),
            Event::Intent(p) => to_value(p),
            Event::Result(p) => to_value(p),
            Event::RunObserved(p) => to_value(p),
            Event::ArtifactInventory(p) => to_value(p),
            Event::Deleted(p) => to_value(p),
            Event::Intake(p) => to_value(p),
            Event::GenerationOpened(p) => to_value(p),
            Event::Lifecycle(p) => to_value(p),
            Event::Inputs(p) => to_value(p),
            Event::Attention(p) => to_value(p),
            Event::EvidenceCollected(p) => to_value(p),
            Event::EvidenceFailed(p) => to_value(p),
            Event::PrivacyFlagMissing(p) => to_value(p),
            Event::Unknown { raw, .. } => raw.clone(),
        };
        let wire = Wire {
            schema_version: self.schema_version,
            seq: self.seq,
            timestamp: self.timestamp,
            hire_id: self.hire_id.clone(),
            generation: self.generation,
            event_type: self.event.event_type().to_owned(),
            payload,
        };
        serde_json::to_string(&wire).expect("journal record serializes")
    }

    /// Parses one line. `line` is the 1-based line number for errors.
    pub fn parse_line(text: &str, line: usize) -> Result<Self, JournalError> {
        let malformed = |message: String| JournalError::Malformed { line, message };
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| malformed(e.to_string()))?;
        if let Some(v) = value.get("schema_version").and_then(|v| v.as_u64())
            && v > u64::from(SCHEMA_VERSION)
        {
            return Err(JournalError::UnsupportedVersion {
                line,
                found: u32::try_from(v).unwrap_or(u32::MAX),
            });
        }
        let wire: Wire = serde_json::from_value(value).map_err(|e| malformed(e.to_string()))?;
        let p = wire.payload;
        let typed = |r: Result<Event, serde_json::Error>| r.map_err(|e| malformed(e.to_string()));
        let event = match wire.event_type.as_str() {
            "received" => typed(serde_json::from_value(p).map(Event::Received))?,
            "decision" => typed(serde_json::from_value(p).map(Event::Decision))?,
            "intent" => typed(serde_json::from_value(p).map(Event::Intent))?,
            "result" => typed(serde_json::from_value(p).map(Event::Result))?,
            "run_observed" => typed(serde_json::from_value(p).map(Event::RunObserved))?,
            "artifact_inventory" => typed(serde_json::from_value(p).map(Event::ArtifactInventory))?,
            "deleted" => typed(serde_json::from_value(p).map(Event::Deleted))?,
            "intake" => typed(serde_json::from_value(p).map(Event::Intake))?,
            "generation_opened" => typed(serde_json::from_value(p).map(Event::GenerationOpened))?,
            "lifecycle" => typed(serde_json::from_value(p).map(Event::Lifecycle))?,
            "inputs" => typed(serde_json::from_value(p).map(Event::Inputs))?,
            "attention" => typed(serde_json::from_value(p).map(Event::Attention))?,
            "evidence_collected" => typed(serde_json::from_value(p).map(Event::EvidenceCollected))?,
            "evidence_failed" => typed(serde_json::from_value(p).map(Event::EvidenceFailed))?,
            "privacy_flag_missing" => {
                typed(serde_json::from_value(p).map(Event::PrivacyFlagMissing))?
            }
            other => Event::Unknown {
                event_type: other.to_owned(),
                raw: p,
            },
        };
        Ok(Record {
            schema_version: wire.schema_version,
            seq: wire.seq,
            timestamp: wire.timestamp,
            hire_id: wire.hire_id,
            generation: wire.generation,
            event,
        })
    }
}

fn to_value<T: Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("journal payload serializes")
}

/// What reading a journal found.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalRead {
    pub records: Vec<Record>,
    /// Intents without a result, in journal order.
    pub open_intents: Vec<Intent>,
    /// 1-based number of a trailing line without its newline that did not
    /// parse (a write cut short); it is not part of `records`.
    pub truncated_line: Option<usize>,
    /// Whether any line was written without the generation field (an S2
    /// journal).
    pub s2_shaped: bool,
    /// The seqs of the records whose lines carried no generation field:
    /// an open intent on one of them predates this version and has no
    /// snapshot to resend from (`s2_pending`, design Migration Plan).
    pub s2_seqs: BTreeSet<u64>,
}

/// Reads journal text. A trailing line without a newline that does not parse
/// is reported as truncated; any other malformed line is an error, as is a
/// newer schema version or a `seq` that does not increase by one from 1.
/// The operations these records settle: every intent with a result, and
/// a run start the engine definitely refused. S2 left such a run intent
/// open and wrote a decline whose detail starts with `run start refused`;
/// nothing runs for it, so it is closed (design, Migration Plan: a decided
/// S2 journal needs nothing).
pub fn closed_operations(records: &[Record]) -> BTreeSet<Uuid> {
    let mut done: BTreeSet<Uuid> = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Result(res) => Some(res.operation_id),
            _ => None,
        })
        .collect();
    for (i, r) in records.iter().enumerate() {
        if let Event::Intent(start) = &r.event
            && start.kind == "run_start"
            && records[i + 1..].iter().any(|later| {
                later.generation == r.generation
                    && matches!(&later.event, Event::Intent(d)
                        if d.kind == "market_decline"
                            && d.refs.get("detail").is_some_and(|t| t.starts_with("run start refused")))
            })
        {
            done.insert(start.operation_id);
        }
    }
    done
}

pub fn read(text: &str) -> Result<JournalRead, JournalError> {
    let mut records = Vec::new();
    let mut truncated_line = None;
    let mut s2_shaped = false;
    let mut s2_seqs: BTreeSet<u64> = BTreeSet::new();
    let ends_with_newline = text.is_empty() || text.ends_with('\n');
    let lines: Vec<&str> = text.split_terminator('\n').collect();
    let last = lines.len();
    for (i, raw) in lines.iter().enumerate() {
        let line = i + 1;
        let s2_line = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .is_some_and(|v| v.get("generation").is_none());
        if s2_line {
            s2_shaped = true;
        }
        match Record::parse_line(raw, line) {
            Ok(r) => {
                if s2_line {
                    s2_seqs.insert(r.seq);
                }
                let previous = records.last().map_or(0, |p: &Record| p.seq);
                if r.seq != previous + 1 {
                    return Err(JournalError::Sequence {
                        line,
                        previous,
                        found: r.seq,
                    });
                }
                records.push(r);
            }
            Err(JournalError::Malformed { .. }) if line == last && !ends_with_newline => {
                truncated_line = Some(line);
            }
            Err(e) => return Err(e),
        }
    }
    let done = closed_operations(&records);
    let open_intents = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Intent(i) if !done.contains(&i.operation_id) => Some(i.clone()),
            _ => None,
        })
        .collect();
    Ok(JournalRead {
        records,
        open_intents,
        truncated_line,
        s2_shaped,
        s2_seqs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn rec(seq: u64, event: Event) -> Record {
        Record {
            schema_version: SCHEMA_VERSION,
            seq,
            timestamp: datetime!(2026-11-09 12:00:00 UTC),
            hire_id: "hire-1".into(),
            generation: 1,
            event,
        }
    }

    fn intent(op: Uuid) -> Intent {
        Intent {
            operation_id: op,
            kind: "run_start".into(),
            execution: 1,
            request_digest: sha256_hex(b"req"),
            refs: BTreeMap::from([("playbook".into(), "smoke".into())]),
        }
    }

    fn result(op: Uuid) -> OpResult {
        OpResult {
            operation_id: op,
            outcome: "ok".into(),
            external_ids: BTreeMap::from([("run_id".into(), "r1".into())]),
        }
    }

    fn join(records: &[Record]) -> String {
        records.iter().map(|r| r.to_line() + "\n").collect()
    }

    #[test]
    fn safe_id_keeps_safe_chars_and_stays_distinct() {
        let a = safe_id("a/b");
        let b = safe_id("a_b");
        assert_ne!(a, b);
        assert!(a.starts_with("a_b-") && b.starts_with("a_b-"));
        assert_eq!(safe_id("a/b"), a, "stable");
        let evil = safe_id("../../etc/passwd");
        assert!(!evil.contains('/') && !evil.contains(".."), "{evil}");
        assert!(
            evil.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
        let p = journal_path(Path::new("/s"), "../x");
        assert_eq!(p.parent().unwrap().parent().unwrap(), Path::new("/s/hires"));
        assert_eq!(p.file_name().unwrap(), "journal.jsonl");
    }

    #[test]
    fn safe_id_is_short_enough_for_a_file_name_with_a_wide_suffix() {
        for id in ["a".repeat(1000), "é".repeat(400), "x/".repeat(300)] {
            let s = safe_id(&id);
            assert!(s.len() <= 100, "{} bytes", s.len());
            let (_, suffix) = s.rsplit_once('-').unwrap();
            assert!(suffix.len() >= 16, "{suffix}");
            assert!(suffix.bytes().all(|b| b.is_ascii_hexdigit()), "{suffix}");
        }
        // Long ids that share the readable prefix stay distinct.
        let a = safe_id(&format!("{}A", "p".repeat(500)));
        let b = safe_id(&format!("{}B", "p".repeat(500)));
        assert_ne!(a, b);
    }

    #[test]
    fn round_trip_keeps_envelope_and_payload() {
        let op = Uuid::now_v7();
        let all = vec![
            rec(1, Event::Received(Received::from_brief("Write a note"))),
            rec(
                2,
                Event::Decision(Decision {
                    decision: "accept".into(),
                    reason: String::new(),
                }),
            ),
            rec(3, Event::Intent(intent(op))),
            rec(4, Event::Result(result(op))),
            rec(
                5,
                Event::RunObserved(RunObserved {
                    attempts: Vec::new(),
                    run_id: "r1".into(),
                    execution: 1,
                    status: "succeeded".into(),
                    playbook_version: Some("1.0.0".into()),
                    cost_usd: Some("0.0123".into()),
                    cost_complete: Some(true),
                    budget_usd: Some("2.60".into()),
                }),
            ),
            rec(
                6,
                Event::ArtifactInventory(ArtifactInventory {
                    files: vec![FileRef {
                        name: "deliverable.md".into(),
                        size: 3,
                        sha256: sha256_hex(b"abc"),
                    }],
                }),
            ),
            rec(
                7,
                Event::Deleted(Deleted {
                    what: vec!["worktree".into()],
                    reason: "retention".into(),
                }),
            ),
        ];
        for r in &all {
            let line = r.to_line();
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            for k in [
                "schema_version",
                "seq",
                "timestamp",
                "hire_id",
                "generation",
                "event_type",
                "payload",
            ] {
                assert!(v.get(k).is_some(), "{k} missing in {line}");
            }
            assert_eq!(v["timestamp"], "2026-11-09T12:00:00Z");
            assert_eq!(&Record::parse_line(&line, 1).unwrap(), r);
        }
        let read = read(&join(&all)).unwrap();
        assert_eq!(read.records, all);
        assert!(read.open_intents.is_empty());
        assert_eq!(read.truncated_line, None);
    }

    /// The new event types of design 2 round-trip, and a terminal
    /// `run_observed` carries its cost fields.
    #[test]
    fn the_new_event_types_round_trip() {
        let all = vec![
            rec(
                1,
                Event::Intake(Intake {
                    check: "scope".into(),
                    outcome: "passed".into(),
                    reason: String::new(),
                    within_first_hour: true,
                }),
            ),
            rec(
                2,
                Event::GenerationOpened(GenerationOpened {
                    generation: 2,
                    kind: "rework".into(),
                    key: sha256_hex(b"key"),
                    feedback_sha256: Some(sha256_hex(b"use a table")),
                    feedback_bytes: Some(11),
                }),
            ),
            rec(
                3,
                Event::Lifecycle(Lifecycle {
                    status: "accepted".into(),
                    cancel_reason: None,
                    finalized_at: Some("2026-11-10T12:00:00Z".into()),
                }),
            ),
            rec(
                4,
                Event::Inputs(Inputs {
                    files: vec![InputFile {
                        attachment_id: "att-1".into(),
                        path: "inputs/00-a.pdf".into(),
                        bytes: 3,
                        sha256: sha256_hex(b"pdf"),
                    }],
                }),
            ),
            rec(
                5,
                Event::Attention(Attention {
                    code: "start_unresolved".into(),
                    detail: "the execution lookup found nothing".into(),
                }),
            ),
            rec(
                6,
                Event::Attention(Attention {
                    code: "none".into(),
                    detail: String::new(),
                }),
            ),
            rec(
                7,
                Event::EvidenceCollected(EvidenceCollected {
                    phase: "start".into(),
                    model: "Qwen/Qwen3.8-27B".into(),
                    sha256: "ab".repeat(32),
                }),
            ),
            rec(
                8,
                Event::EvidenceFailed(EvidenceFailed {
                    phase: "end".into(),
                    model: "Qwen/Qwen3.8-27B".into(),
                    reason: "503 Service Unavailable".into(),
                }),
            ),
        ];
        for r in &all {
            assert_eq!(&Record::parse_line(&r.to_line(), 1).unwrap(), r);
        }
        let read = read(&join(&all)).unwrap();
        assert_eq!(read.records, all);
    }

    /// The evidence events of design 6.2 are written under their flat
    /// event type names and carry no report body.
    #[test]
    fn evidence_events_use_their_flat_names() {
        let r = rec(
            1,
            Event::EvidenceCollected(EvidenceCollected {
                phase: "start".into(),
                model: "m".into(),
                sha256: "00".into(),
            }),
        );
        let line = r.to_line();
        assert!(
            line.contains(r#""event_type":"evidence_collected""#),
            "{line}"
        );
        assert!(!line.contains("body"), "{line}");
        let f = rec(
            2,
            Event::EvidenceFailed(EvidenceFailed {
                phase: "end".into(),
                model: "m".into(),
                reason: "timeout".into(),
            }),
        );
        assert!(f.to_line().contains(r#""event_type":"evidence_failed""#));
    }

    /// The new intent kinds of design 2 round-trip through the existing
    /// intent record.
    #[test]
    fn the_new_intent_kinds_round_trip() {
        for kind in ["run_abort", "market_progress", "scope_check"] {
            let mut i = intent(Uuid::now_v7());
            i.kind = kind.into();
            let r = rec(1, Event::Intent(i));
            assert_eq!(Record::parse_line(&r.to_line(), 1).unwrap(), r);
        }
    }

    /// Every record written now carries its generation in the envelope;
    /// a record without one (S2) reads as generation 1.
    #[test]
    fn a_record_without_generation_reads_as_generation_one() {
        let s2_line = r#"{"schema_version":1,"seq":1,"timestamp":"2026-11-09T12:00:00Z","hire_id":"h","event_type":"received","payload":{"brief_sha256":"ab","brief_bytes":2}}"#;
        let r = Record::parse_line(s2_line, 1).unwrap();
        assert_eq!(
            r.generation, 1,
            "a record without a generation reads as generation 1"
        );
        assert!(matches!(r.event, Event::Received(_)));
        // A record this version writes carries the field.
        let own = rec(1, Event::Received(Received::from_brief("b")));
        let v: serde_json::Value = serde_json::from_str(&own.to_line()).unwrap();
        assert_eq!(v["generation"], 1);
    }

    /// S2-shaped journals (no generations) read as generation 1, one
    /// fixture per S2 shape (design, Migration Plan); the open intents of
    /// the open shapes are reported for serve.

    #[test]
    fn s2_shaped_journals_read_as_generation_one() {
        let received = r#"{"schema_version":1,"seq":1,"timestamp":"2026-11-09T12:00:00Z","hire_id":"h","event_type":"received","payload":{"brief_sha256":"ab","brief_bytes":2}}"#;
        let run_intent = r#"{"schema_version":1,"seq":2,"timestamp":"2026-11-09T12:00:01Z","hire_id":"h","event_type":"intent","payload":{"operation_id":"01900000-0000-7000-8000-000000000001","kind":"run_start","execution":1,"request_digest":"cd","refs":{"execution_id":"x-1"}}}"#;
        let submit_intent = r#"{"schema_version":1,"seq":2,"timestamp":"2026-11-09T12:00:01Z","hire_id":"h","event_type":"intent","payload":{"operation_id":"01900000-0000-7000-8000-000000000002","kind":"market_submit","execution":1,"request_digest":"ef","refs":{}}}"#;
        let submit_result = r#"{"schema_version":1,"seq":3,"timestamp":"2026-11-09T12:00:02Z","hire_id":"h","event_type":"result","payload":{"operation_id":"01900000-0000-7000-8000-000000000002","outcome":"submitted","external_ids":{}}}"#;
        let decision = r#"{"schema_version":1,"seq":4,"timestamp":"2026-11-09T12:00:03Z","hire_id":"h","event_type":"decision","payload":{"decision":"delivered","reason":""}}"#;
        let running_obs = r#"{"schema_version":1,"seq":3,"timestamp":"2026-11-09T12:00:02Z","hire_id":"h","event_type":"run_observed","payload":{"run_id":"r1","execution":1,"status":"running","playbook_version":null}}"#;
        let terminal_obs = r#"{"schema_version":1,"seq":2,"timestamp":"2026-11-09T12:00:02Z","hire_id":"h","event_type":"run_observed","payload":{"run_id":"r1","execution":1,"status":"succeeded","playbook_version":"1.0.123"}}"#;
        let shapes: [(&str, &str, usize); 5] = [
            (
                "decided",
                &[received, submit_intent, submit_result, decision].join("\n"),
                0,
            ),
            (
                "running",
                &[received, run_intent, running_obs].join("\n"),
                1,
            ),
            (
                "terminal undecided",
                &[received, terminal_obs].join("\n"),
                0,
            ),
            ("open run_start", &[received, run_intent].join("\n"), 1),
            (
                "open market_submit",
                &[received, submit_intent].join("\n"),
                1,
            ),
        ];
        for (name, text, open) in shapes {
            let j = read(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                j.records.iter().all(|r| r.generation == 1),
                "{name}: every record belongs to generation 1"
            );
            assert_eq!(j.open_intents.len(), open, "{name}");
        }
    }

    #[test]
    fn unknown_event_type_is_kept_raw() {
        let line = r#"{"schema_version":1,"seq":1,"timestamp":"2026-11-09T12:00:00Z","hire_id":"h","event_type":"future_thing","payload":{"x":[1,2]}}"#;
        let r = Record::parse_line(line, 1).unwrap();
        assert_eq!(
            r.event,
            Event::Unknown {
                event_type: "future_thing".into(),
                raw: serde_json::json!({"x":[1,2]})
            }
        );
        assert_eq!(Record::parse_line(&r.to_line(), 1).unwrap(), r);
    }

    #[test]
    fn newer_schema_version_is_refused_naming_the_line() {
        let ok = rec(1, Event::Received(Received::from_brief("b"))).to_line();
        let newer = r#"{"schema_version":2,"seq":2,"timestamp":"2026-11-09T12:00:00Z","hire_id":"h","event_type":"received","payload":{}}"#;
        let err = read(&format!("{ok}\n{newer}\n")).unwrap_err();
        assert_eq!(err, JournalError::UnsupportedVersion { line: 2, found: 2 });
        assert!(err.to_string().contains("line 2"));
    }

    #[test]
    fn open_intent_is_reported_and_matched_ones_are_not() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let text = join(&[
            rec(1, Event::Intent(intent(a))),
            rec(2, Event::Result(result(a))),
            rec(3, Event::Intent(intent(b))),
        ]);
        let r = read(&text).unwrap();
        assert_eq!(r.open_intents, vec![intent(b)]);
    }

    #[test]
    fn truncated_trailing_line_is_reported() {
        let full = join(&[rec(1, Event::Received(Received::from_brief("b")))]);
        let cut = r#"{"schema_version":1,"seq":2,"timest"#;
        let r = read(&format!("{full}{cut}")).unwrap();
        assert_eq!(r.records.len(), 1);
        assert_eq!(r.truncated_line, Some(2));
    }

    #[test]
    fn malformed_interior_line_is_an_error() {
        let a = rec(1, Event::Received(Received::from_brief("b"))).to_line();
        let c = rec(2, Event::Received(Received::from_brief("c"))).to_line();
        let err = read(&format!("{a}\nnot json\n{c}\n")).unwrap_err();
        assert!(matches!(err, JournalError::Malformed { line: 2, .. }));
        // A broken last line that does end with a newline is not a cut write.
        let err = read(&format!("{a}\nnot json\n")).unwrap_err();
        assert!(matches!(err, JournalError::Malformed { line: 2, .. }));
    }

    #[test]
    fn seq_must_increase_by_one_from_one() {
        let a = rec(2, Event::Received(Received::from_brief("b"))).to_line();
        assert!(matches!(
            read(&format!("{a}\n")).unwrap_err(),
            JournalError::Sequence { line: 1, .. }
        ));
        // A gap is an error too: seq is consecutive, not just increasing.
        let one = rec(1, Event::Received(Received::from_brief("b"))).to_line();
        let three = rec(3, Event::Received(Received::from_brief("c"))).to_line();
        assert!(matches!(
            read(&format!("{one}\n{three}\n")).unwrap_err(),
            JournalError::Sequence {
                line: 2,
                previous: 1,
                found: 3
            }
        ));
    }

    #[test]
    fn brief_digest_matches_a_known_sha256_vector() {
        // FIPS 180-2 test vector, independent of the code under test.
        assert_eq!(
            Received::from_brief("abc").brief_sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn received_record_holds_digest_and_length_not_the_brief() {
        let brief = "Translate my secret plan for the moon base into French";
        let r = rec(1, Event::Received(Received::from_brief(brief)));
        let line = r.to_line();
        assert!(!line.contains("moon base"), "{line}");
        assert!(!line.contains(brief));
        assert!(line.contains(&sha256_hex(brief.as_bytes())));
        assert!(line.contains(&format!("\"brief_bytes\":{}", brief.len())));
    }
}

/// A journal writer: appends one record per call with consecutive `seq`,
/// fsyncing the file (and, on creation, the directory) before it returns,
/// so an external write that a record announces always follows a durable
/// record. A journal with a truncated tail refuses writing.
#[derive(Debug)]
pub struct Writer {
    file: std::fs::File,
    path: PathBuf,
    hire_id: String,
    /// The clock that stamps every record (S6 design 2).
    clock: crate::clock::Clock,
    /// The generation every appended record carries; 1 until serve sets
    /// it (design 2).
    generation: u32,
    next_seq: u64,
    /// Set after a failed write or sync: the seq the failed record got may
    /// have reached the disk partially, so a retry through this writer
    /// could append a duplicate seq. The writer refuses further appends;
    /// the journal is reopened instead.
    poisoned: bool,
}

/// Why a journal cannot be opened for writing.
#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("journal {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "journal {path}: line {line} is truncated by an earlier crash; repair or remove the journal by hand"
    )]
    TruncatedTail { path: PathBuf, line: usize },
    #[error("journal: {0}")]
    Read(JournalError),
    #[error("journal {path}: another writer holds it in this process")]
    Locked { path: PathBuf },
    #[error(
        "journal {path}: an earlier append failed mid-record; reopen the journal instead of appending"
    )]
    Poisoned { path: PathBuf },
}

/// The journals this process holds open: one writer at a time per hire.
static OPEN_JOURNALS: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::new(BTreeSet::new());

fn acquire_journal_lock(path: &Path) -> Result<(), WriterError> {
    let mut set = OPEN_JOURNALS.lock().unwrap_or_else(|e| e.into_inner());
    if !set.insert(path.to_path_buf()) {
        return Err(WriterError::Locked {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn release_journal_lock(path: &Path) {
    let mut set = OPEN_JOURNALS.lock().unwrap_or_else(|e| e.into_inner());
    set.remove(path);
}

impl Writer {
    /// Opens (or creates) the journal at `path` for appending, continuing
    /// the sequence of the records already on disk. One writer at a time
    /// per hire within this process: a second open fails naming the file.
    /// Every record is stamped with `clock` (serve passes its round clock,
    /// every other caller [`crate::clock::Clock::real`]).
    pub fn open(
        path: PathBuf,
        hire_id: &str,
        clock: crate::clock::Clock,
    ) -> Result<Self, WriterError> {
        acquire_journal_lock(&path)?;
        match Self::open_inner(path.clone(), hire_id, clock) {
            Ok(w) => Ok(w),
            Err(e) => {
                release_journal_lock(&path);
                Err(e)
            }
        }
    }

    fn open_inner(
        path: PathBuf,
        hire_id: &str,
        clock: crate::clock::Clock,
    ) -> Result<Self, WriterError> {
        if let Some(dir) = path.parent() {
            create_dir_durable(dir).map_err(|source| WriterError::Io {
                path: path.clone(),
                source,
            })?;
            // Hire journals carry hire ids and decline reasons: they stay
            // private to the owner (dir 0700, file 0600).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            }
        }
        let existing = std::fs::read_to_string(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                WriterError::Io {
                    path: path.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "not readable yet"),
                }
            } else {
                WriterError::Io {
                    path: path.clone(),
                    source,
                }
            }
        });
        let next_seq = match existing {
            Ok(text) => {
                if text.is_empty() {
                    1
                } else {
                    let r = read(&text).map_err(WriterError::Read)?;
                    if let Some(line) = r.truncated_line {
                        return Err(WriterError::TruncatedTail {
                            path: path.clone(),
                            line,
                        });
                    }
                    r.records.last().map_or(1, |r| r.seq + 1)
                }
            }
            Err(WriterError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                1
            }
            Err(e) => return Err(e),
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| WriterError::Io {
                path: path.clone(),
                source,
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        if next_seq == 1 {
            // A fresh journal: make the hire directory entry and the new
            // entry in hires/ durable before the first record is announced
            // to anyone.
            // A failed sync is returned, never ignored (C16).
            if let Some(dir) = path.parent() {
                sync_dir(dir).map_err(|source| WriterError::Io {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        Ok(Self {
            file,
            path,
            hire_id: hire_id.to_owned(),
            clock,
            generation: 1,
            next_seq,
            poisoned: false,
        })
    }

    /// The next `seq` a record will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// The hire id this writer appends under.
    pub fn hire_id(&self) -> &str {
        &self.hire_id
    }

    /// The generation appended records carry.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Sets the generation the following records carry (design 2: the
    /// records of the opened generation carry its number).
    pub fn set_generation(&mut self, generation: u32) {
        self.generation = generation;
    }

    /// Appends one record: the line and the fsync happen before the call
    /// returns, so any external write the record announces is made only
    /// after the record is on disk.
    pub fn append(&mut self, event: Event) -> Result<u64, WriterError> {
        if self.poisoned {
            return Err(WriterError::Poisoned {
                path: self.path.clone(),
            });
        }
        let record = Record {
            schema_version: SCHEMA_VERSION,
            seq: self.next_seq,
            timestamp: self.clock.now(),
            hire_id: self.hire_id.clone(),
            generation: self.generation,
            event,
        };
        let mut line = record.to_line();
        line.push('\n');
        {
            use std::io::Write;
            if let Err(source) = self.file.write_all(line.as_bytes()) {
                self.poisoned = true;
                return Err(WriterError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        }
        if let Err(source) = self.file.sync_all() {
            self.poisoned = true;
            return Err(WriterError::Io {
                path: self.path.clone(),
                source,
            });
        }
        self.next_seq += 1;
        Ok(record.seq)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        release_journal_lock(&self.path);
    }
}

/// The generation directory of a hire: `<state_dir>/hires/<safe>/gen-<n>`.
pub fn gen_dir(state_dir: &Path, hire_id: &str, generation: u32) -> PathBuf {
    state_dir
        .join("hires")
        .join(safe_id(hire_id))
        .join(format!("gen-{generation}"))
}

/// The submission snapshot of a generation (`gen-<n>/submission.md`).
pub const SUBMISSION_SNAPSHOT: &str = "submission.md";

/// The run request snapshot of one execution (`gen-<n>/run-<e>.json`).
pub fn run_snapshot_name(execution: u32) -> String {
    format!("run-{execution}.json")
}

/// A snapshot read failure.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("snapshot {path}: its SHA-256 is {found}, not the journaled digest")]
    Mismatch { path: PathBuf, found: String },
}

/// Whether `name` is a safe snapshot file name (no separators, no leading
/// dot, plain characters only).
fn valid_snapshot_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The path of one snapshot inside a generation directory.
pub fn snapshot_path(state_dir: &Path, hire_id: &str, generation: u32, name: &str) -> PathBuf {
    gen_dir(state_dir, hire_id, generation).join(name)
}

/// Writes one snapshot durably and privately: the bytes go to a temporary
/// name in the generation directory (created 0700), are flushed to disk,
/// and the file (mode 0600) is renamed to its final name and the directory
/// entry made durable, all before the call returns, so the intent that
/// names the snapshot and the external write that follows it can only
/// happen after the snapshot is complete on disk (design, On-disk state).
pub fn write_snapshot(
    state_dir: &Path,
    hire_id: &str,
    generation: u32,
    name: &str,
    bytes: &[u8],
) -> Result<PathBuf, SnapshotError> {
    if !valid_snapshot_name(name) {
        return Err(SnapshotError::Io {
            path: PathBuf::from(name),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a safe snapshot name",
            ),
        });
    }
    let dir = gen_dir(state_dir, hire_id, generation);
    // Each new directory entry is made durable through its parent before
    // the snapshot is written, so a durable intent can never name a
    // snapshot whose directory vanished at a power loss (C16).
    create_dir_durable(&dir).map_err(|source| SnapshotError::Io {
        path: dir.clone(),
        source,
    })?;
    let final_path = dir.join(name);
    let tmp = dir.join(format!(
        ".{name}.{}.tmp",
        &Uuid::now_v7().simple().to_string()[..12]
    ));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|source| SnapshotError::Io {
            path: tmp.clone(),
            source,
        })?;
        f.write_all(bytes).map_err(|source| SnapshotError::Io {
            path: tmp.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        f.sync_all().map_err(|source| SnapshotError::Io {
            path: tmp.clone(),
            source,
        })?;
    }
    std::fs::rename(&tmp, &final_path).map_err(|source| SnapshotError::Io {
        path: final_path.clone(),
        source,
    })?;
    sync_dir(&dir).map_err(|source| SnapshotError::Io {
        path: dir.clone(),
        source,
    })?;
    Ok(final_path)
}

/// Flushes a directory's entries to disk.
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// The directories whose entry this process made durable in their parent
/// (created and synced, or found and synced again, R4-6).
fn settled_dirs() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

/// Re-establishes the parent barrier of directories that already exist
/// (R4-6): existence alone never proves that the `mkdir` that made them
/// was followed by its parent's sync, since a crash may have come in
/// between. Walking up from `dir`, each directory owned by this user
/// (every directory [`create_dir_durable`] makes is) whose entry this
/// process has not settled yet gets its parent synced, once per process;
/// a directory owned by someone else was not made here, and the walk
/// stops at it.
fn settle_existing(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let me = rustix::process::geteuid().as_raw();
        let mut cur = dir;
        loop {
            if settled_dirs()
                .lock()
                .map(|s| s.contains(cur))
                .unwrap_or(false)
            {
                return Ok(());
            }
            if std::fs::symlink_metadata(cur)?.uid() != me {
                return Ok(());
            }
            let Some(parent) = cur.parent().filter(|p| !p.as_os_str().is_empty()) else {
                return Ok(());
            };
            sync_dir(parent)?;
            if let Ok(mut s) = settled_dirs().lock() {
                s.insert(cur.to_path_buf());
            }
            cur = parent;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Creates `dir` and every missing ancestor one at a time, each private
/// (0700), syncing each new entry's parent so the entry itself is
/// durable (C16). The ancestors that already exist pass the same barrier
/// once per process (R4-6), so a directory a crash left unsynced is made
/// durable before anything is written under it. Returns how many
/// directories it created; any failure is returned, never ignored.
pub fn create_dir_durable(dir: &Path) -> std::io::Result<usize> {
    let mut missing = Vec::new();
    let mut cur = dir;
    loop {
        match std::fs::symlink_metadata(cur) {
            Ok(m) if m.is_dir() => break,
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    format!("{} is not a directory", cur.display()),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cur.to_path_buf());
                match cur.parent() {
                    Some(p) if !p.as_os_str().is_empty() => cur = p,
                    _ => break,
                }
            }
            Err(e) => return Err(e),
        }
    }
    let created = missing.len();
    // `cur` is the deepest ancestor that exists (or the empty root).
    if !missing.contains(&cur.to_path_buf()) {
        settle_existing(cur)?;
    }
    for d in missing.into_iter().rev() {
        match std::fs::create_dir(&d) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && d.is_dir() => continue,
            Err(e) => return Err(e),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))?;
        }
        if let Some(parent) = d.parent().filter(|p| !p.as_os_str().is_empty()) {
            sync_dir(parent)?;
        }
        if let Ok(mut s) = settled_dirs().lock() {
            s.insert(d.clone());
        }
    }
    Ok(created)
}

/// Reads one snapshot's bytes.
pub fn read_snapshot(
    state_dir: &Path,
    hire_id: &str,
    generation: u32,
    name: &str,
) -> Result<Vec<u8>, SnapshotError> {
    if !valid_snapshot_name(name) {
        return Err(SnapshotError::Io {
            path: PathBuf::from(name),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a safe snapshot name",
            ),
        });
    }
    let path = snapshot_path(state_dir, hire_id, generation, name);
    std::fs::read(&path).map_err(|source| SnapshotError::Io { path, source })
}

/// Reads one snapshot and verifies it against the digest the intent
/// journaled. A mismatch is an error, never bytes the caller could send
/// by mistake.
pub fn verify_snapshot(
    state_dir: &Path,
    hire_id: &str,
    generation: u32,
    name: &str,
    digest: &str,
) -> Result<Vec<u8>, SnapshotError> {
    let bytes = read_snapshot(state_dir, hire_id, generation, name)?;
    let found = sha256_hex(&bytes);
    if found != digest {
        return Err(SnapshotError::Mismatch {
            path: snapshot_path(state_dir, hire_id, generation, name),
            found,
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn digest(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    /// A written snapshot reads back whole and verifies against its
    /// digest (the ordering against the external call is proven in serve:
    /// `a_rework_submit_follows_its_snapshot_and_intent`).
    #[test]
    fn a_snapshot_round_trips_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"# deliverable".to_vec();
        let path = write_snapshot(dir.path(), "hire-1", 2, SUBMISSION_SNAPSHOT, &bytes).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        // Recovery reads the same bytes back and verifies them.
        assert_eq!(
            verify_snapshot(
                dir.path(),
                "hire-1",
                2,
                SUBMISSION_SNAPSHOT,
                &digest(&bytes)
            )
            .unwrap(),
            bytes
        );
    }

    /// A snapshot whose digest differs from the intent's is refused: the
    /// caller gets an error instead of bytes it could send (serve maps the
    /// error to an attention code and sends nothing, design 3).
    #[test]
    fn a_mismatching_snapshot_digest_is_reported_and_nothing_can_be_sent() {
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(dir.path(), "hire-1", 1, SUBMISSION_SNAPSHOT, b"# original").unwrap();
        // The file changed after the intent was written (tampered or a
        // stale snapshot): the digest does not match.
        std::fs::write(
            snapshot_path(dir.path(), "hire-1", 1, SUBMISSION_SNAPSHOT),
            b"# tampered",
        )
        .unwrap();
        let err = verify_snapshot(
            dir.path(),
            "hire-1",
            1,
            SUBMISSION_SNAPSHOT,
            &digest(b"# original"),
        )
        .unwrap_err();
        assert!(matches!(err, SnapshotError::Mismatch { .. }), "{err}");
        assert!(err.to_string().contains("not the journaled digest"));
    }

    /// The generation directory is 0700, snapshots are 0600, and a write
    /// leaves no temporary file behind.
    #[test]
    fn snapshot_directories_and_files_are_private_and_writes_are_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_snapshot(dir.path(), "hire-1", 1, "run-1.json", b"{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dmode = std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(dmode & 0o777, 0o700, "the generation directory");
            let fmode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(fmode & 0o777, 0o600, "the snapshot file");
        }
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no temporary file stays behind");
    }

    /// C16: every directory a snapshot needs is created one component at
    /// a time with its parent synced, private (0700), and a component that
    /// cannot be a directory fails the write before any intent could
    /// follow it.
    #[test]
    fn snapshot_directories_are_created_durably_and_failures_propagate() {
        let dir = tempfile::tempdir().unwrap();
        // The hire directory does not exist yet.
        write_snapshot(dir.path(), "h1", 2, SUBMISSION_SNAPSHOT, b"x").unwrap();
        let g = gen_dir(dir.path(), "h1", 2);
        assert!(g.join(SUBMISSION_SNAPSHOT).is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&g).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let created = create_dir_durable(&dir.path().join("a/b/c")).unwrap();
        assert_eq!(created, 3);
        std::fs::write(dir.path().join("file"), b"").unwrap();
        assert!(create_dir_durable(&dir.path().join("file/sub")).is_err());
        assert!(
            write_snapshot(&dir.path().join("file"), "h1", 1, SUBMISSION_SNAPSHOT, b"x").is_err()
        );
    }

    /// A name that could escape the generation directory is refused.
    #[test]
    fn an_unsafe_snapshot_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../evil", "", "..", ".hidden", "a/b", "a\\b"] {
            assert!(
                write_snapshot(dir.path(), "hire-1", 1, name, b"x").is_err(),
                "{name} must be refused"
            );
        }
        // The safe names the adapter uses are accepted.
        for name in ["submission.md", "run-1.json", "run-12.json"] {
            assert!(write_snapshot(dir.path(), "hire-1", 1, name, b"x").is_ok());
        }
    }

    /// A missing snapshot is a not-found, not bytes.
    #[test]
    fn a_missing_snapshot_is_an_io_error_not_empty_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_snapshot(dir.path(), "hire-1", 1, SUBMISSION_SNAPSHOT).unwrap_err();
        match err {
            SnapshotError::Io { source, .. } => {
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("{other}"),
        }
    }
}

#[cfg(test)]
mod durable_dir_tests {
    use super::*;

    fn chmod(p: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// R4-6: a directory a crash left between its `mkdir` and its
    /// parent's sync is not taken as durable: the next creation in a new
    /// process syncs its parent again (here the parent cannot be opened,
    /// so the barrier fails instead of passing on existence).
    #[test]
    fn an_existing_directory_still_passes_its_parent_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("hires");
        let hire = parent.join("h1");
        std::fs::create_dir_all(&hire).unwrap();
        chmod(&parent, 0o300);
        let r = create_dir_durable(&hire);
        chmod(&parent, 0o755);
        assert!(r.is_err(), "existence was taken as a completed barrier");
        assert_eq!(create_dir_durable(&hire).unwrap(), 0);
    }

    /// R4-6: a journal whose hire directory a crash left unsynced is not
    /// written to before that directory's entry in `hires/` is durable.
    #[test]
    fn a_writer_never_appends_under_an_unsynced_hire_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hires").join("h2").join("journal.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let hires = dir.path().join("hires");
        chmod(&hires, 0o300);
        let r = Writer::open(path.clone(), "h2", crate::clock::Clock::real());
        chmod(&hires, 0o755);
        assert!(r.is_err(), "the writer opened before the barrier");
        drop(r);
        let mut w = Writer::open(path, "h2", crate::clock::Clock::real()).unwrap();
        w.append(Event::Received(Received::from_brief("b")))
            .unwrap();
    }
}

#[cfg(test)]
mod writer_tests {
    use super::*;

    fn dir_journal() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hires").join("h-1").join("journal.jsonl");
        (dir, path)
    }

    fn received(seq_hint: &str) -> Event {
        Event::Received(Received::from_brief(seq_hint))
    }

    #[test]
    fn appends_consecutive_seqs_and_survives_a_reopen() {
        let (_dir, path) = dir_journal();
        let mut w = Writer::open(path.clone(), "h-1", crate::clock::Clock::real()).unwrap();
        assert_eq!(w.next_seq(), 1);
        assert_eq!(w.append(received("a")).unwrap(), 1);
        assert_eq!(w.append(received("b")).unwrap(), 2);
        drop(w);
        let mut w = Writer::open(path.clone(), "h-1", crate::clock::Clock::real()).unwrap();
        assert_eq!(w.next_seq(), 3, "reopen continues the sequence");
        assert_eq!(w.append(received("c")).unwrap(), 3);
        let text = std::fs::read_to_string(&path).unwrap();
        let r = read(&text).unwrap();
        assert_eq!(r.records.len(), 3);
        assert_eq!(r.records[2].seq, 3);
    }

    #[test]
    fn a_truncated_tail_refuses_writing() {
        let (_dir, path) = dir_journal();
        let mut w = Writer::open(path.clone(), "h-1", crate::clock::Clock::real()).unwrap();
        w.append(received("a")).unwrap();
        drop(w);
        // Simulate a crash mid-write: cut the last line's final bytes.
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, &text[..text.len() - 5]).unwrap();
        let err = Writer::open(path.clone(), "h-1", crate::clock::Clock::real()).unwrap_err();
        assert!(
            matches!(err, WriterError::TruncatedTail { line: 1, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("truncated"));
    }

    #[test]
    fn an_empty_journal_starts_at_one() {
        let (_dir, path) = dir_journal();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        let mut w = Writer::open(path, "h-1", crate::clock::Clock::real()).unwrap();
        assert_eq!(w.next_seq(), 1);
        w.append(Event::Decision(Decision {
            decision: "accept".into(),
            reason: String::new(),
        }))
        .unwrap();
    }

    /// One writer at a time per hire within the process.
    #[test]
    fn a_second_writer_on_the_same_journal_is_refused() {
        let (_dir, path) = dir_journal();
        let w = Writer::open(path.clone(), "h-1", crate::clock::Clock::real()).unwrap();
        let err = Writer::open(path.clone(), "h-1", crate::clock::Clock::real()).unwrap_err();
        assert!(matches!(err, WriterError::Locked { .. }), "{err}");
        drop(w);
        // After the first writer is gone, opening works again.
        let mut w = Writer::open(path, "h-1", crate::clock::Clock::real()).unwrap();
        w.append(received("later")).unwrap();
    }

    /// A broken interior line is an error, not silently overwritten.
    #[test]
    fn a_broken_journal_refuses_writing() {
        let (_dir, path) = dir_journal();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json at all\n").unwrap();
        let err = Writer::open(path, "h-1", crate::clock::Clock::real()).unwrap_err();
        assert!(matches!(err, WriterError::Read(_)), "{err}");
    }
}

#[cfg(test)]
mod poison_tests {
    use super::*;

    /// A failed append (write or sync) poisons the writer: a retry through
    /// the same writer can never append a duplicate seq (ocr2 12). The
    /// failure is provoked by pointing the writer at a read-only file
    /// descriptor, whose writes fail.
    #[test]
    fn a_failed_append_poisons_the_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h-1").join("journal.jsonl");
        let mut w = Writer::open(path, "h-1", crate::clock::Clock::real()).unwrap();
        let mut read_only = std::fs::File::open("/dev/null").unwrap();
        std::mem::swap(&mut w.file, &mut read_only);
        let err = w
            .append(received_event("first"))
            .expect_err("the write must fail on a read-only descriptor");
        assert!(matches!(err, WriterError::Io { .. }), "{err}");
        // The retry is refused without touching the file again.
        let err = w
            .append(received_event("second"))
            .expect_err("the writer is poisoned");
        assert!(matches!(err, WriterError::Poisoned { .. }), "{err}");
        assert!(err.to_string().contains("reopen"));
    }

    fn received_event(seq_hint: &str) -> Event {
        Event::Received(Received::from_brief(seq_hint))
    }
}
