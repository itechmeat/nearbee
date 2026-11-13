//! The market's wire types: exact Rust shapes of the OpenAPI components
//! (`docs/reference/market-openapi.json`) that the client sends or parses.
//! Names, casing, nesting, required-versus-optional and enums follow the
//! components; the Nearbee model types in `nearbee_core::market` are mapped
//! from these in the crate root.
//!
//! Mixed casing is the API's own: assignment, job, message and file
//! components use camelCase on the wire, agent and pricing-plan components
//! use snake_case. The shapes are checked component by component in the
//! contract test (`tests/contract.rs`), which builds instances from the
//! schema and deserializes them with these types; the live captures of
//! task 3.4 are still to confirm them against the live API.
//!
//! The shared fixture builder behind that contract test and the wiremock
//! tests lives in the dev-only crate `nearbee-market-fixture`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums (the components' closed sets)
// ---------------------------------------------------------------------------

/// `AssignmentStatus`: the per-worker assignment lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentStatus {
    InProgress,
    Submitted,
    Accepted,
    Disputed,
    DisputeResolved,
    Cancelled,
}

/// `JobKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Standard,
    Instant,
    Competition,
}

/// `JobStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Open,
    Filling,
    InProgress,
    Completed,
    Closed,
}

/// `MessageOrigin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    Direct,
    Axi,
    Mcp,
    Platform,
}

/// `ListingStatus` (shared by agents and pricing plans).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingStatus {
    Draft,
    Live,
    Paused,
    Archived,
}

/// `PricingModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingModel {
    PerCall,
    PerTask,
    Subscription,
}

/// `WorkerCancelReason`: the two reasons a worker may give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerCancelReason {
    Rejected,
    Failed,
}

/// `Category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Automation,
    Creative,
    CustomerSupport,
    Data,
    DevTools,
    Events,
    Finance,
    LegalCompliance,
    Marketing,
    Operations,
    Research,
    Sales,
    Travel,
    Other,
}

/// `Runtime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Http,
    Managed,
    Human,
}

/// `AgentAvailability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAvailability {
    Available,
    Paused,
    Suspended,
    UnavailableFunding,
}

/// The exact wire spelling of a serialized enum value.
pub fn wire_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Parses an RFC 3339 wire timestamp.
pub fn wire_time(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

// ---------------------------------------------------------------------------
// Responses the client parses
// ---------------------------------------------------------------------------

/// One attachment as every read surface reports it (`AttachmentBrief`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentBrief {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
    pub created_at: String,
    pub download_url: String,
}

/// The latest thread message, embedded in an assignment row
/// (`LatestMessageBrief`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestMessageBrief {
    pub sender_side: String,
    pub origin: MessageOrigin,
    pub body: String,
    pub truncated: bool,
    pub attachments: Vec<AttachmentBrief>,
    pub created_at: String,
}

/// The job brief embedded in each assignment row (`WorkerJobBrief`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerJobBrief {
    pub job_id: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub job_kind: JobKind,
    pub status: JobStatus,
    pub attachments: Vec<AttachmentBrief>,
    pub entries_close_at: Option<String>,
}

/// One assignment row of a job (`JobAssignment`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAssignment {
    pub assignment_id: String,
    pub job_id: String,
    pub worker_agent_id: String,
    pub status: AssignmentStatus,
    pub escrow_amount: String,
    pub escrow_token: String,
    pub sla_seconds: i32,
    pub sla_deadline_at: String,
    pub acceptance_window_days: i32,
    pub created_at: String,
    pub updated_at: String,
    pub bid_id: Option<String>,
    pub cancel_detail: Option<String>,
    pub cancel_reason: Option<String>,
    pub deliverable_hash: Option<String>,
    pub deliverable_url: Option<String>,
    pub finalized_at: Option<String>,
    pub started_at: Option<String>,
    pub submitted_at: Option<String>,
}

/// One row of the worker's assignment list (`MyAssignmentItem`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyAssignmentItem {
    pub assignment: JobAssignment,
    pub job: WorkerJobBrief,
    pub latest_message: Option<LatestMessageBrief>,
    pub auto_accept_at: Option<String>,
}

/// One thread message (`ThreadMessage`: `AssignmentMessage` flattened
/// with the `attachments` of its allOf branch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessageView {
    pub message_id: String,
    pub assignment_id: String,
    pub sender_account_id: String,
    #[serde(default)]
    pub sender_agent_id: Option<String>,
    pub origin: MessageOrigin,
    pub body: String,
    pub created_at: String,
    pub attachments: Vec<AttachmentBrief>,
}

/// Response envelope of `GET /v1/assignments/{id}/messages`
/// (`ListMessagesResponse`; only what Nearbee reads).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMessagesResponse {
    pub messages: Vec<ThreadMessageView>,
    pub total: i64,
}

/// Response envelope of `GET /v1/agents/me/assignments`
/// (`MyAssignmentsResponse`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MyAssignmentsResponse {
    pub assignments: Vec<MyAssignmentItem>,
}

/// One required slot of an agent (`RequiredSlot`; only the required
/// properties, which is all Nearbee reads).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequiredSlot {
    pub key: String,
    pub label: String,
}

/// The public view of an agent (`AgentPublic`; every required property,
/// plus the optional ones Nearbee patches and reads back).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentPublic {
    pub agent_id: String,
    pub owner_account_id: String,
    pub handle: String,
    pub name: String,
    pub category: Category,
    pub tags: Vec<String>,
    pub runtime: Runtime,
    pub listing_status: ListingStatus,
    pub delivered_jobs: i32,
    pub sla_seconds: i32,
    pub required_slots: Vec<RequiredSlot>,
    pub funding_paused: bool,
    pub availability: AgentAvailability,
    pub created_at: String,
    pub updated_at: String,
    pub description: Option<String>,
    pub bio_markdown: Option<String>,
}

/// The full agent view returned to the owning account (`OwnerAgentView`:
/// the `Agent` component's required fields flattened with the owner-only
/// additions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnerAgentView {
    pub agent_id: String,
    pub owner_account_id: String,
    pub handle: String,
    pub name: String,
    pub category: Category,
    pub tags: Vec<String>,
    pub runtime: Runtime,
    pub webhook_enabled: bool,
    pub webhook_failure_count: i32,
    pub listing_status: ListingStatus,
    pub delivered_jobs: i32,
    pub sla_seconds: i32,
    pub required_slots: Vec<RequiredSlot>,
    pub created_at: String,
    pub updated_at: String,
    pub skill_ids: Vec<String>,
    pub unverified_skill_ids: Vec<String>,
    pub stale_unverified_skill_ids: Vec<String>,
    pub funding_paused: bool,
    pub availability: AgentAvailability,
    pub available_now: bool,
    pub active_jobs: i64,
    pub allow_admin_test_runs: bool,
    pub description: Option<String>,
    pub bio_markdown: Option<String>,
}

/// What `GET /v1/agents/{id_or_handle}` answers (`AgentView`: one of
/// `OwnerAgentView`, `AdminAgentView`, `AgentPublic`; tried in that
/// order, most specific first).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum AgentView {
    Owner(Box<OwnerAgentView>),
    Admin(Box<AdminAgentView>),
    Public(Box<AgentPublic>),
}

/// What a platform admin reads: the public view plus the owner's consent
/// to admin test runs (`AdminAgentView`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminAgentView {
    #[serde(flatten)]
    pub public: AgentPublic,
    pub allow_admin_test_runs: bool,
}

/// One pricing plan of an agent (`PricingPlan`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingPlan {
    pub pricing_plan_id: String,
    pub agent_id: String,
    pub model: PricingModel,
    pub price_amount: String,
    pub is_default: bool,
    pub listing_status: ListingStatus,
    pub created_at: String,
    pub updated_at: String,
    pub sla_seconds: Option<i32>,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

/// Answer of `POST /v1/files` (`ReserveFileResponse`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveFileResponse {
    pub file_id: String,
    pub upload_url: String,
    pub direct_upload_url: String,
    pub byte_size: i64,
    pub expires_in_secs: i64,
    pub file_url: Option<String>,
}

/// Answer of `POST /v1/files/{id}/complete` (`CompleteFileResponse`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteFileResponse {
    pub file_id: String,
    pub filename: String,
    pub byte_size: i64,
    pub sha256: String,
    pub content_type: String,
}

// ---------------------------------------------------------------------------
// Requests the client sends
// ---------------------------------------------------------------------------

/// `SendMessageRequest`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
}

/// `SubmitRequest`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitRequest {
    pub deliverable_markdown: String,
    pub files: Vec<String>,
}

/// `ReportProgressRequest`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportProgressRequest {
    pub message: String,
    /// `int32`, 0 to 100; left out when the worker cannot tell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<i32>,
}

/// `DeclineAssignmentRequest`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclineRequest {
    pub reason: Option<WorkerCancelReason>,
    pub detail: Option<String>,
}

/// `ReserveFileRequest`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveFileRequest {
    pub filename: String,
    pub byte_size: i64,
    pub link: bool,
}

/// `CreatePricingPlanRequest`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatePricingPlanRequest {
    pub model: PricingModel,
    pub price_amount: String,
    /// `int32` on the wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// `PatchAgentRequest` (the fields Nearbee patches; everything else is
/// left out and keeps its value; `listing_status` is never sent).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PatchAgentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio_markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// `int32` on the wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_seconds: Option<i32>,
}
