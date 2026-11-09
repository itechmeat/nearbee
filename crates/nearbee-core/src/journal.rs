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

#[derive(Serialize, Deserialize)]
struct Wire {
    schema_version: u32,
    seq: u64,
    #[serde(with = "time::serde::rfc3339")]
    timestamp: OffsetDateTime,
    hire_id: String,
    event_type: String,
    payload: serde_json::Value,
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
            Event::Unknown { raw, .. } => raw.clone(),
        };
        let wire = Wire {
            schema_version: self.schema_version,
            seq: self.seq,
            timestamp: self.timestamp,
            hire_id: self.hire_id.clone(),
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
}

/// Reads journal text. A trailing line without a newline that does not parse
/// is reported as truncated; any other malformed line is an error, as is a
/// newer schema version or a `seq` that does not increase by one from 1.
pub fn read(text: &str) -> Result<JournalRead, JournalError> {
    let mut records = Vec::new();
    let mut truncated_line = None;
    let ends_with_newline = text.is_empty() || text.ends_with('\n');
    let lines: Vec<&str> = text.split_terminator('\n').collect();
    let last = lines.len();
    for (i, raw) in lines.iter().enumerate() {
        let line = i + 1;
        match Record::parse_line(raw, line) {
            Ok(r) => {
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
    let done: BTreeSet<Uuid> = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Result(res) => Some(res.operation_id),
            _ => None,
        })
        .collect();
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
                    run_id: "r1".into(),
                    execution: 1,
                    status: "succeeded".into(),
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
