//! The market seam: row and request types, error classes, row
//! classification and the `Market` trait. Implemented by `nearbee-market`;
//! the core depends only on this module.
//!
//! Facts in the types follow `docs/market.md` and the vendored worker skill.
//! The product name is NEAR-bound; these types are not.

use std::fmt;
use std::future::Future;

use serde::Deserialize;
use time::OffsetDateTime;

/// Id of a webhook delivery, from the `X-Market-Delivery` header
/// (webhook API v1.1). Type and parse only in this stage: serve polls, no
/// webhook code exists.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeliveryId(String);

/// Longest delivery id kept, in bytes.
pub const DELIVERY_ID_LIMIT: usize = 200;

impl DeliveryId {
    /// Accepts a trimmed header value: not empty, no whitespace, at most
    /// [`DELIVERY_ID_LIMIT`] bytes. Nothing else is promised about the
    /// shape, so a market-side format change does not drop deliveries.
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("X-Market-Delivery is empty".into());
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err("X-Market-Delivery carries whitespace".into());
        }
        if trimmed.len() > DELIVERY_ID_LIMIT {
            return Err(format!(
                "X-Market-Delivery is longer than {DELIVERY_ID_LIMIT} bytes"
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeliveryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An assignment status the stage acts on; unknown statuses are kept and are
/// never acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    InProgress,
    Submitted,
    Accepted,
    Cancelled,
    Disputed,
    DisputeResolved,
    Unknown(String),
}

impl RowStatus {
    /// Maps the market's status word (`docs/market.md`, worker lifecycle).
    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "submitted" => Self::Submitted,
            "accepted" => Self::Accepted,
            "cancelled" => Self::Cancelled,
            "disputed" => Self::Disputed,
            "dispute_resolved" => Self::DisputeResolved,
            other => Self::Unknown(other.to_owned()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::InProgress => "in_progress",
            Self::Submitted => "submitted",
            Self::Accepted => "accepted",
            Self::Cancelled => "cancelled",
            Self::Disputed => "disputed",
            Self::DisputeResolved => "dispute_resolved",
            Self::Unknown(s) => s,
        }
    }

    /// The row will not change any more.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Accepted | Self::Cancelled | Self::DisputeResolved
        )
    }
}

impl fmt::Display for RowStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One attachment as every market read surface reports it
/// (`AttachmentBrief`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    /// The declared size in bytes, as the market reports it (an `int64` on
    /// the wire; checked against the declared values before any download).
    pub byte_size: i64,
    pub sha256: String,
    /// The download path: an absolute URL on the market origin or a
    /// relative one to resolve against it.
    pub download_url: String,
}

/// The latest thread message embedded in an assignment row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub body: String,
    #[serde(default)]
    pub sender_side: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub created_at: Option<OffsetDateTime>,
    /// Whether the market cut the body; the full text comes from the
    /// thread read.
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

/// One thread message as the messages listing answers it
/// (`ThreadMessage`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessage {
    pub message_id: String,
    pub body: String,
    /// The agent id when the worker side authored it; `None` for the
    /// buyer or a system author.
    #[serde(default)]
    pub sender_agent_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub created_at: Option<OffsetDateTime>,
}

/// One assignment row: Nearbee's own model, not the wire shape. The
/// market answers `{"assignments": [MyAssignmentItem]}` with nested
/// `assignment`, `job`, `latestMessage` and `autoAcceptAt`;
/// `nearbee-market` maps each item onto this row. The flat camelCase
/// `Deserialize` is only a convenience for fakes and tests. Classification uses
/// `deliverable_url` and `submitted_at` only; `started_at` is kept for serve
/// to see that the owner's web session already started the hire.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub submitted_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub deliverable_url: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub sla_deadline_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub auto_accept_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub cancel_reason: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub created_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub updated_at: Option<OffsetDateTime>,
    /// When the hire closed (`accepted`, `dispute_resolved`).
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub finalized_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub sla_seconds: Option<u64>,
    /// The escrow award in USD, a decimal string as the market reports it.
    #[serde(default)]
    pub escrow_amount: Option<String>,
    #[serde(default)]
    pub escrow_token: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// The buyer's brief: data, never instructions to Nearbee.
    #[serde(default)]
    pub description: Option<String>,
    /// The files the buyer attached to the job.
    #[serde(default)]
    pub job_attachments: Vec<Attachment>,
    #[serde(default)]
    pub latest_message: Option<Message>,
}

/// What kind of work a row asks for, decided from `deliverable_url` and
/// `submitted_at` only. `started_at` is never used: the owner's web session
/// or a webhook `2xx` may have stamped start before Nearbee saw the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Neither delivery field set: first-time work.
    FirstTime,
    /// Both set: request-changes rework; the feedback is the newest message.
    Rework,
    /// `deliverable_url` set, `submitted_at` empty: a dispute redo.
    Redo,
    /// `submitted_at` set with `deliverable_url` empty: an undefined state.
    /// Serve never acts on it.
    Unknown,
}

impl Row {
    pub fn status(&self) -> RowStatus {
        RowStatus::parse(self.status.as_deref().unwrap_or_default())
    }

    pub fn kind(&self) -> RowKind {
        match (&self.deliverable_url, &self.submitted_at) {
            (None, None) => RowKind::FirstTime,
            (Some(_), Some(_)) => RowKind::Rework,
            (Some(_), None) => RowKind::Redo,
            // Undefined: never treated as first-time work, so serve cannot
            // redo delivered work.
            (None, Some(_)) => RowKind::Unknown,
        }
    }
}

/// A write the market refused, or a failure of a read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarketError {
    /// 401: the token is revoked or wrong. Never retried; serve stops this
    /// agent and tells the operator.
    #[error("unauthorized: {detail}")]
    Unauthorized { detail: String },
    /// Another 4xx, with the status and the market's machine code.
    #[error("refused ({status} {code}): {detail}")]
    Refused {
        status: u16,
        code: String,
        detail: String,
    },
    /// A read failed on transport or a retryable status after its retries.
    /// `retry_after_secs` is the delay a 503 named, when it did.
    #[error("read failed: {detail}")]
    RetryableRead {
        detail: String,
        retry_after_secs: Option<u64>,
    },
    /// A write was sent and its outcome is unknown (timeout, reset, 5xx,
    /// malformed success, redirect). Resolve from the journal.
    #[error("write outcome unknown: {detail}")]
    WriteOutcomeUnknown { detail: String },
    /// An answer the client cannot understand.
    #[error("protocol error: {detail}")]
    Protocol { detail: String },
}

/// A thread message send. The id makes a retried send idempotent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRequest {
    pub body: String,
    /// 1 to 128 characters, unique per send.
    pub client_request_id: String,
}

/// A progress report. At most one per assignment per 30 seconds; a too-soon
/// report answers `409 progress_report_too_soon` with a delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressRequest {
    /// 1 to 280 characters.
    pub message: String,
    pub percent: Option<u8>,
}

/// The outcome of a progress report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressOutcome {
    Reported,
    /// `409 progress_report_too_soon`: send no progress for this row
    /// before the named delay (design 9.5: the client returns the delay
    /// instead of waiting).
    TooSoon {
        retry_after_secs: u64,
    },
}

/// A submit. `files` holds file URLs of confirmed uploads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitRequest {
    pub markdown: String,
    pub files: Vec<String>,
}

/// A decline reason. `rejected` within the first hour does not count
/// against the success rate; after work started, `failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineReason {
    Rejected,
    Failed,
}

impl DeclineReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DeclineReason::Rejected => "rejected",
            DeclineReason::Failed => "failed",
        }
    }
}

impl fmt::Display for DeclineReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclineRequest {
    pub reason: DeclineReason,
    pub detail: Option<String>,
}

/// The outcome of a decline, after the client's own repeat rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineOutcome {
    /// The market cancelled the row.
    Done,
    /// The market refused the decline and a fresh read does not show the row
    /// cancelled.
    Refused,
}

/// The fields of an agent a publish patches. A field left `None` is not
/// sent, so it keeps its value. `listing_status` is deliberately not a
/// field: publishing never changes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub bio_markdown: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub sla_seconds: Option<u64>,
}

/// An agent record as the commands need it: Nearbee's own model, not a
/// wire shape. `nearbee-market` maps it from the `AgentView` components
/// (`OwnerAgentView`, `AdminAgentView`, `AgentPublic`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecord {
    pub agent_id: String,
    pub handle: Option<String>,
    pub name: Option<String>,
    pub listing_status: Option<String>,
}

/// One pricing plan of an agent: Nearbee's own model, mapped by
/// `nearbee-market` from the `PricingPlan` component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingPlan {
    pub pricing_plan_id: String,
    /// The plan model; v1 takes only `per_call`.
    pub model: Option<String>,
    /// Decimal string in USD.
    pub price_amount: String,
    pub sla_seconds: Option<u64>,
    pub is_default: bool,
    pub listing_status: Option<String>,
}

/// A pricing plan to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPricingPlan {
    /// Decimal string in USD.
    pub price_usd: String,
    pub sla_seconds: u64,
    pub display_name: Option<String>,
}

/// A file to upload (the reserve, PUT, complete trio).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The link a confirmed upload serves from, with what the market stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedFile {
    pub file_id: String,
    pub file_url: String,
    /// The stored size and SHA-256 from the complete answer
    /// (`CompleteFileResponse`). `None` when the complete answered `409
    /// upload_not_pending`: the same-origin upload sink had already stored
    /// the bytes (measured 2026-11-10), so the market reported no digest.
    pub stored: Option<StoredFile>,
}

/// A reserved, linked file slot (`POST /v1/files` with `link: true`): its
/// id, the URL its bytes go to and the public link it will serve from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedFile {
    pub file_id: String,
    pub upload_url: String,
    pub file_url: String,
}

/// The size and SHA-256 (lowercase hex) the market stored for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    pub byte_size: u64,
    pub sha256: String,
}

/// Everything Nearbee needs from an agent market. Implemented by
/// `nearbee-market`; the core depends only on this trait.
pub trait Market: Send + Sync {
    /// Lists the agent's assignments in every status, following paging.
    fn assignments(&self) -> impl Future<Output = Result<Vec<Row>, MarketError>> + Send;
    /// Declares work started. Resent once on an unknown outcome by the
    /// client; `WriteOutcomeUnknown` means serve resolves from its journal.
    fn start(&self, row_id: &str) -> impl Future<Output = Result<(), MarketError>> + Send;
    /// Sends one thread message (idempotent through the client request id).
    fn message(
        &self,
        row_id: &str,
        req: &MessageRequest,
    ) -> impl Future<Output = Result<(), MarketError>> + Send;
    /// Reports progress; `TooSoon` returns the `409
    /// progress_report_too_soon` delay for the caller to schedule, and the
    /// client never repeats the call itself.
    fn progress(
        &self,
        row_id: &str,
        req: &ProgressRequest,
    ) -> impl Future<Output = Result<ProgressOutcome, MarketError>> + Send;
    /// Submits the deliverable. Resent once, identically, on an unknown
    /// outcome.
    fn submit(
        &self,
        row_id: &str,
        req: &SubmitRequest,
    ) -> impl Future<Output = Result<(), MarketError>> + Send;
    /// Hands the assignment back. Repeated at most once; the outcome says
    /// whether the row is cancelled.
    fn decline(
        &self,
        row_id: &str,
        req: &DeclineRequest,
    ) -> impl Future<Output = Result<DeclineOutcome, MarketError>> + Send;
    /// The public record of one agent (no auth).
    fn agent(&self, handle: &str) -> impl Future<Output = Result<AgentRecord, MarketError>> + Send;
    /// Patches the agent; never `listing_status`.
    fn patch_agent(
        &self,
        agent_id: &str,
        patch: &AgentPatch,
    ) -> impl Future<Output = Result<AgentRecord, MarketError>> + Send;
    /// Lists the agent's pricing plans.
    fn pricing_plans(
        &self,
        agent_id: &str,
    ) -> impl Future<Output = Result<Vec<PricingPlan>, MarketError>> + Send;
    /// Creates a per-call pricing plan.
    fn create_pricing_plan(
        &self,
        agent_id: &str,
        plan: &NewPricingPlan,
    ) -> impl Future<Output = Result<PricingPlan, MarketError>> + Send;
    /// Makes one plan the default.
    fn make_default(
        &self,
        agent_id: &str,
        plan_id: &str,
    ) -> impl Future<Output = Result<(), MarketError>> + Send;
    /// Reserves a linked slot for one file (`POST /v1/files`). A reserve
    /// whose answer carries no `fileUrl` is deleted before the error is
    /// reported.
    fn reserve_file(
        &self,
        file: &UploadFile,
    ) -> impl Future<Output = Result<ReservedFile, MarketError>> + Send;
    /// PUTs the exact bytes to a reserved slot and completes it. A failure
    /// deletes the reserved file (best effort) before it is reported.
    /// `Ok(None)` when the complete answered `409 upload_not_pending`.
    fn complete_file(
        &self,
        reserved: &ReservedFile,
        file: &UploadFile,
    ) -> impl Future<Output = Result<Option<StoredFile>, MarketError>> + Send;
    /// Uploads one file (reserve, PUT the exact bytes, complete) and returns
    /// its public link. Serve calls the two steps itself, so it can journal
    /// the file id between them.
    fn upload_file(
        &self,
        file: &UploadFile,
    ) -> impl Future<Output = Result<UploadedFile, MarketError>> + Send {
        async move {
            let reserved = self.reserve_file(file).await?;
            let stored = self.complete_file(&reserved, file).await?;
            Ok(UploadedFile {
                file_id: reserved.file_id,
                file_url: reserved.file_url,
                stored,
            })
        }
    }
    /// Deletes a file no job uses yet, its link included (`DELETE
    /// /v1/files/{id}`, idempotent; an unknown file counts as gone). A
    /// bound file answers `409 file_bound`, a `Refused`.
    fn delete_file(&self, file_id: &str) -> impl Future<Output = Result<(), MarketError>> + Send;
    /// Lists an assignment's thread messages, newest first, stopping at the
    /// first page that holds a message created at or before `until`
    /// (design 9.3: used to read the full body of a truncated latest
    /// message).
    fn messages(
        &self,
        row_id: &str,
        until: Option<OffsetDateTime>,
    ) -> impl Future<Output = Result<Vec<ThreadMessage>, MarketError>> + Send;
    /// The configured market origin (`scheme://host[:port]`), which every
    /// attachment download must stay on (spec `hire-attachments`).
    fn origin(&self) -> String;
    /// Downloads an attachment by streaming it into `out`: the bearer only
    /// to the configured origin, no redirect, at most `cap` bytes (design
    /// 9.4). Returns the byte count; the caller verifies size and hash.
    fn download(
        &self,
        path: &str,
        cap: u64,
        out: &mut (dyn std::io::Write + Send),
    ) -> impl Future<Output = Result<u64, MarketError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str, fields: &[(&str, serde_json::Value)]) -> Row {
        let mut v = serde_json::json!({ "id": "a1", "status": status });
        if let serde_json::Value::Object(m) = &mut v {
            for (k, val) in fields {
                m.insert((*k).to_owned(), val.clone());
            }
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn classification_uses_only_delivery_fields() {
        // Both empty: first-time, also when the owner already started it.
        let r = row(
            "in_progress",
            &[("startedAt", serde_json::json!("2026-11-09T10:00:00Z"))],
        );
        assert_eq!(r.kind(), RowKind::FirstTime);
        // Both set: rework.
        let r = row(
            "in_progress",
            &[
                ("deliverableUrl", serde_json::json!("https://m/f/abc")),
                ("submittedAt", serde_json::json!("2026-11-09T10:00:00Z")),
            ],
        );
        assert_eq!(r.kind(), RowKind::Rework);
        // Deliverable set, submitted empty: dispute redo.
        let r = row(
            "in_progress",
            &[("deliverableUrl", serde_json::json!("https://m/f/abc"))],
        );
        assert_eq!(r.kind(), RowKind::Redo);
        // submittedAt set alone is an undefined state: never first-time
        // work, so serve cannot redo delivered work.
        let r = row(
            "in_progress",
            &[("submittedAt", serde_json::json!("2026-11-09T10:00:00Z"))],
        );
        assert_eq!(r.kind(), RowKind::Unknown);
        assert_ne!(r.kind(), RowKind::FirstTime);
    }

    #[test]
    fn row_parses_camel_case_and_maps_status() {
        let r: Row = serde_json::from_str(
            r#"{"id":"a1","status":"cancelled","cancelReason":"sla_backstop",
                "slaDeadlineAt":"2026-11-10T12:00:00Z","autoAcceptAt":"2026-11-16T12:00:00Z",
                "latestMessage":{"body":"hi","senderSide":"buyer"}}"#,
        )
        .unwrap();
        assert_eq!(r.status(), RowStatus::Cancelled);
        assert!(r.status().is_terminal());
        assert_eq!(r.cancel_reason.as_deref(), Some("sla_backstop"));
        assert_eq!(
            r.latest_message.as_ref().unwrap().sender_side.as_deref(),
            Some("buyer")
        );
        // Unknown statuses are kept and never terminal.
        assert!(!RowStatus::parse("queued_by_martians").is_terminal());
        // A changed source gives a different version, an unchanged one the
        // same (the republish rule); proven in nearbee-apb's install tests.
    }

    #[test]
    fn delivery_id_parse_rules() {
        assert_eq!(
            DeliveryId::parse("  d4f0-abc  ").unwrap().as_str(),
            "d4f0-abc"
        );
        assert!(DeliveryId::parse("").is_err());
        assert!(DeliveryId::parse("   ").is_err());
        assert!(DeliveryId::parse("has space").is_err());
        let long = "a".repeat(201);
        assert!(DeliveryId::parse(&long).is_err());
        assert!(DeliveryId::parse(&"a".repeat(200)).is_ok());
        let id = DeliveryId::parse("x").unwrap();
        assert_eq!(id.to_string(), "x");
        let _ = DELIVERY_ID_LIMIT;
    }
}
