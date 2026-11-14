//! Contract test: the client's wire types against the vendored OpenAPI
//! (`docs/reference/market-openapi.json`).
//!
//! Responses: for every component the client parses, a minimal instance
//! built from the schema's required properties, and one with the optional
//! properties the client reads, deserialize with the client's type.
//! Requests: every body the client sends serializes to property names the
//! component has, with every required one present and each value of the
//! schema's type (enums from the schema's set). The operation table pins
//! which component each endpoint takes and answers, so a spec change that
//! moves an endpoint to another component fails here too.

use nearbee_market::wire;
use nearbee_market_fixture::{component_schema, instance, instance_with, spec};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn parse<T: DeserializeOwned>(component: &str, v: Value) -> T {
    serde_json::from_value(v.clone())
        .unwrap_or_else(|e| panic!("{component} does not parse: {e}\n{v:#}"))
}

/// The `$ref` component name of an operation's JSON request body or
/// response, `None` when the operation defines none.
fn op_component(path: &str, method: &str, response: Option<&str>) -> Option<String> {
    let op = &spec()["paths"][path][method];
    assert!(op.is_object(), "{method} {path} is not in the OpenAPI");
    let content = match response {
        Some(code) => &op["responses"][code]["content"]["application/json"]["schema"],
        None => &op["requestBody"]["content"]["application/json"]["schema"],
    };
    let r = content
        .get("$ref")
        .or_else(|| content.get("items").and_then(|i| i.get("$ref")))?;
    Some(r.as_str()?.rsplit('/').next()?.to_owned())
}

/// One operation: path, method, request component, success code, answer
/// component.
type Operation = (
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
);

/// The operations the client calls, with the request and response
/// components its wire types are written for (`None`: no body).
#[test]
fn every_operation_the_client_calls_uses_the_component_its_wire_type_models() {
    let table: &[Operation] = &[
        (
            "/v1/agents/me/assignments",
            "get",
            None,
            "200",
            Some("MyAssignmentsResponse"),
        ),
        (
            "/v1/assignments/{id}/start",
            "post",
            None,
            "200",
            Some("StartAssignmentResponse"),
        ),
        (
            "/v1/assignments/{id}/messages",
            "post",
            Some("SendMessageRequest"),
            "201",
            Some("ThreadMessage"),
        ),
        (
            "/v1/assignments/{id}/progress",
            "post",
            Some("ReportProgressRequest"),
            "200",
            Some("WorkerProgressReport"),
        ),
        (
            "/v1/assignments/{id}/submit",
            "post",
            Some("SubmitRequest"),
            "200",
            Some("JobAssignment"),
        ),
        (
            "/v1/assignments/{id}/decline",
            "post",
            Some("DeclineAssignmentRequest"),
            "200",
            Some("JobAssignment"),
        ),
        (
            "/v1/files",
            "post",
            Some("ReserveFileRequest"),
            "201",
            Some("ReserveFileResponse"),
        ),
        (
            "/v1/files/{file_id}/complete",
            "post",
            None,
            "200",
            Some("CompleteFileResponse"),
        ),
        ("/v1/files/{file_id}", "delete", None, "204", None),
        (
            "/v1/agents/{id_or_handle}",
            "get",
            None,
            "200",
            Some("AgentView"),
        ),
        (
            "/v1/agents/{agent_id}",
            "patch",
            Some("PatchAgentRequest"),
            "200",
            Some("OwnerAgentView"),
        ),
        (
            "/v1/agents/{agent_id}/pricing-plans",
            "get",
            None,
            "200",
            Some("PricingPlan"),
        ),
        (
            "/v1/agents/{agent_id}/pricing-plans",
            "post",
            Some("CreatePricingPlanRequest"),
            "201",
            Some("PricingPlan"),
        ),
        (
            "/v1/pricing-plans/{pricing_plan_id}/make-default",
            "post",
            None,
            "200",
            Some("PricingPlan"),
        ),
        (
            "/v1/jobs/board",
            "get",
            None,
            "200",
            Some("ListBoardJobsResponse"),
        ),
        (
            "/v1/agents/me/bids",
            "get",
            None,
            "200",
            Some("MyBidsResponse"),
        ),
        (
            "/v1/jobs/{id}/bids",
            "post",
            Some("PlaceBidRequest"),
            "201",
            Some("Bid"),
        ),
        (
            "/v1/jobs/{job_id}/attachments",
            "get",
            None,
            "200",
            Some("ListJobAttachmentsResponse"),
        ),
    ];
    for (path, method, req, code, resp) in table {
        assert_eq!(
            op_component(path, method, None).as_deref(),
            *req,
            "{method} {path} request body"
        );
        assert_eq!(
            op_component(path, method, Some(code)).as_deref(),
            *resp,
            "{method} {path} {code} answer"
        );
    }
    // The plan list is an array of plans, not an envelope.
    assert_eq!(
        spec()["paths"]["/v1/agents/{agent_id}/pricing-plans"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["type"],
        "array"
    );
}

/// The complete answer parses with the size and digest serve checks.
#[test]
fn the_complete_answer_parses_with_its_size_and_digest() {
    let c: wire::CompleteFileResponse = parse(
        "CompleteFileResponse",
        instance("CompleteFileResponse", Value::Null),
    );
    assert!(!c.file_id.is_empty());
    assert!(!c.sha256.is_empty());
    assert_eq!(c.byte_size, 0);
}

#[test]
fn the_assignments_envelope_parses_minimal_and_with_the_optionals_nearbee_reads() {
    // Minimal: required properties only (latestMessage and autoAcceptAt
    // absent, every nullable assignment field absent).
    let min = instance("MyAssignmentsResponse", Value::Null);
    let r: wire::MyAssignmentsResponse = parse("MyAssignmentsResponse", min);
    assert_eq!(r.assignments.len(), 1);
    assert!(r.assignments[0].latest_message.is_none());
    assert!(r.assignments[0].assignment.started_at.is_none());

    // With the optional properties the mapping reads.
    let assignment = instance_with(
        "JobAssignment",
        &[
            "startedAt",
            "submittedAt",
            "deliverableUrl",
            "cancelReason",
            "finalizedAt",
        ],
        json!({ "status": "cancelled" }),
    );
    let item = instance_with(
        "MyAssignmentItem",
        &["latestMessage", "autoAcceptAt"],
        json!({ "assignment": assignment }),
    );
    let r: wire::MyAssignmentsResponse =
        parse("MyAssignmentsResponse", json!({ "assignments": [item] }));
    let i = &r.assignments[0];
    assert_eq!(i.assignment.status, wire::AssignmentStatus::Cancelled);
    assert!(i.assignment.started_at.is_some());
    assert!(i.assignment.deliverable_url.is_some());
    assert!(i.assignment.finalized_at.is_some(), "finalizedAt parses");
    assert!(
        !i.assignment.escrow_amount.is_empty(),
        "escrowAmount is required and parses as its decimal string"
    );
    assert!(i.latest_message.is_some());
    assert!(i.auto_accept_at.is_some());
    // The nested latest message and job carry attachments.
    assert_eq!(i.job.attachments.len(), 1);
    assert_eq!(
        i.latest_message.as_ref().unwrap().attachments.len(),
        1,
        "LatestMessageBrief carries its attachments"
    );
    // `AttachmentBrief` parses with every field Nearbee keeps.
    let a: wire::AttachmentBrief =
        parse("AttachmentBrief", instance("AttachmentBrief", Value::Null));
    assert!(!a.id.is_empty());
    assert!(!a.filename.is_empty());
    assert!(!a.content_type.is_empty());
    assert_eq!(a.byte_size, 0);
    assert!(!a.sha256.is_empty());
    assert!(!a.download_url.is_empty());
    // `LatestMessageBrief` keeps truncated and the timestamps.
    let l: wire::LatestMessageBrief = parse(
        "LatestMessageBrief",
        instance_with("LatestMessageBrief", &["attachments"], Value::Null),
    );
    assert!(!l.truncated);
    assert!(!l.created_at.is_empty());
    assert_eq!(l.attachments.len(), 1);

    // An explicit null in a nullable field parses too.
    let mut nulls = instance("MyAssignmentItem", Value::Null);
    nulls["latestMessage"] = Value::Null;
    nulls["autoAcceptAt"] = Value::Null;
    nulls["assignment"]["startedAt"] = Value::Null;
    let _: wire::MyAssignmentItem = parse("MyAssignmentItem", nulls);
}

#[test]
fn every_agent_view_parses_with_snake_case_names() {
    for (component, want) in [
        ("OwnerAgentView", "owner"),
        ("AdminAgentView", "admin"),
        ("AgentPublic", "public"),
    ] {
        let v = instance(
            component,
            json!({ "agent_id": "a-1", "listing_status": "draft" }),
        );
        let view: wire::AgentView = parse(component, v.clone());
        let got = match &view {
            wire::AgentView::Owner(o) => {
                assert_eq!(o.agent_id, "a-1");
                "owner"
            }
            wire::AgentView::Admin(a) => {
                assert_eq!(a.public.agent_id, "a-1");
                "admin"
            }
            wire::AgentView::Public(p) => {
                assert_eq!(p.agent_id, "a-1");
                assert_eq!(p.listing_status, wire::ListingStatus::Draft);
                "public"
            }
        };
        assert_eq!(got, want, "{component} parsed as the wrong view");
        // With the optional text fields Nearbee patches.
        let full = instance_with(component, &["description", "bio_markdown"], Value::Null);
        let _: wire::AgentView = parse(component, full);
    }
    let owner = instance("OwnerAgentView", Value::Null);
    let _: wire::OwnerAgentView = parse("OwnerAgentView", owner);
    let public = instance("AgentPublic", Value::Null);
    let _: wire::AgentPublic = parse("AgentPublic", public);
}

#[test]
fn a_pricing_plan_parses_minimal_and_with_its_optionals() {
    let min = instance("PricingPlan", Value::Null);
    let p: wire::PricingPlan = parse("PricingPlan", min);
    assert!(p.sla_seconds.is_none());
    let full = instance_with(
        "PricingPlan",
        &["sla_seconds", "display_name", "description"],
        json!({ "pricing_plan_id": "p-1", "price_amount": "0.50", "is_default": true }),
    );
    let p: wire::PricingPlan = parse("PricingPlan", full.clone());
    assert_eq!(p.pricing_plan_id, "p-1");
    assert!(p.is_default);
    assert!(p.sla_seconds.is_some());
    let list: Vec<wire::PricingPlan> = parse("PricingPlan[]", json!([full]));
    assert_eq!(list.len(), 1);
}

#[test]
fn a_reserve_answer_parses_minimal_and_with_its_file_url() {
    let min = instance("ReserveFileResponse", Value::Null);
    let r: wire::ReserveFileResponse = parse("ReserveFileResponse", min);
    assert!(r.file_url.is_none());
    let full = instance_with("ReserveFileResponse", &["fileUrl"], Value::Null);
    let r: wire::ReserveFileResponse = parse("ReserveFileResponse", full);
    assert!(r.file_url.is_some());
}

/// Checks one value against a schema node: type (or null where the
/// schema allows it) and enum membership.
fn check_value(component: &str, name: &str, schema: &Value, v: &Value) {
    let s = resolve_with_null(schema);
    if v.is_null() {
        assert!(
            s.nullable,
            "{component}.{name} is sent as null but the schema does not allow null"
        );
        return;
    }
    if let Some(allowed) = &s.enumeration {
        assert!(
            allowed.contains(v),
            "{component}.{name} = {v} is not in the schema's enum {allowed:?}"
        );
    }
    let ok = match s.ty.as_deref() {
        Some("string") => v.is_string(),
        Some("integer") => v.is_i64() || v.is_u64(),
        Some("number") => v.is_number(),
        Some("boolean") => v.is_boolean(),
        Some("array") => v.is_array(),
        Some("object") => v.is_object(),
        _ => true,
    };
    assert!(ok, "{component}.{name} = {v} is not of type {:?}", s.ty);
    if let (Some(min), Some(n)) = (s.minimum, v.as_i64()) {
        assert!(n >= min, "{component}.{name} = {n} is below {min}");
    }
    if let (Some(max), Some(n)) = (s.maximum, v.as_i64()) {
        assert!(n <= max, "{component}.{name} = {n} is above {max}");
    }
}

struct Leaf {
    ty: Option<String>,
    nullable: bool,
    enumeration: Option<Vec<Value>>,
    minimum: Option<i64>,
    maximum: Option<i64>,
}

fn resolve_with_null(schema: &Value) -> Leaf {
    let root = spec();
    let mut nullable = false;
    let mut node = schema.clone();
    loop {
        if let Some(r) = node.get("$ref").and_then(Value::as_str) {
            let name = r.rsplit('/').next().unwrap();
            node = root["components"]["schemas"][name].clone();
            continue;
        }
        if let Some(arr) = node.get("oneOf").and_then(Value::as_array).cloned() {
            nullable |= arr.iter().any(|b| b["type"] == "null");
            node = arr
                .into_iter()
                .find(|b| b["type"] != "null")
                .unwrap_or(Value::Null);
            continue;
        }
        break;
    }
    let ty = match node.get("type") {
        Some(Value::String(t)) => Some(t.clone()),
        Some(Value::Array(a)) => {
            nullable |= a.iter().any(|t| t == "null");
            a.iter()
                .filter_map(Value::as_str)
                .find(|t| *t != "null")
                .map(str::to_owned)
        }
        _ => None,
    };
    Leaf {
        ty,
        nullable,
        enumeration: node.get("enum").and_then(Value::as_array).cloned(),
        minimum: node.get("minimum").and_then(Value::as_i64),
        maximum: node.get("maximum").and_then(Value::as_i64),
    }
}

/// Serializes `body` and checks it against `component`: every property
/// exists, every required one is present, every value fits its schema.
fn check_request<T: Serialize>(component: &str, body: &T) {
    let v = serde_json::to_value(body).unwrap();
    let obj = v
        .as_object()
        .unwrap_or_else(|| panic!("{component} body is not an object"));
    let schema = component_schema(spec(), component);
    let props = schema["properties"].as_object().unwrap();
    for (name, value) in obj {
        let prop = props
            .get(name)
            .unwrap_or_else(|| panic!("{component} has no property `{name}` (sent: {v})"));
        check_value(component, name, prop, value);
    }
    for req in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let req = req.as_str().unwrap();
        assert!(
            obj.contains_key(req),
            "{component} requires `{req}`, which the client does not send ({v})"
        );
    }
}

#[test]
fn every_request_body_the_client_sends_fits_its_component() {
    check_request(
        "SendMessageRequest",
        &wire::SendMessageRequest {
            body: "hi".into(),
            client_request_id: Some("cr-1".into()),
        },
    );
    check_request(
        "SubmitRequest",
        &wire::SubmitRequest {
            deliverable_markdown: "# result".into(),
            files: vec!["https://market.near.ai/files/t/a.md".into()],
        },
    );
    for percent in [None, Some(0), Some(100)] {
        check_request(
            "ReportProgressRequest",
            &wire::ReportProgressRequest {
                message: "working".into(),
                percent,
            },
        );
    }
    for reason in [
        wire::WorkerCancelReason::Rejected,
        wire::WorkerCancelReason::Failed,
    ] {
        for detail in [None, Some("why".to_owned())] {
            check_request(
                "DeclineAssignmentRequest",
                &wire::DeclineRequest {
                    reason: Some(reason),
                    detail,
                },
            );
        }
    }
    check_request(
        "ReserveFileRequest",
        &wire::ReserveFileRequest {
            filename: "a.md".into(),
            byte_size: 3,
            link: true,
        },
    );
    check_request(
        "CreatePricingPlanRequest",
        &wire::CreatePricingPlanRequest {
            model: wire::PricingModel::PerCall,
            price_amount: "0.50".into(),
            sla_seconds: Some(86_400),
            display_name: Some("Repo audit".into()),
        },
    );
    // Every field Nearbee patches, and the empty patch.
    check_request(
        "PatchAgentRequest",
        &wire::PatchAgentRequest {
            name: Some("Repo audit".into()),
            description: Some("Audits a repository.".into()),
            bio_markdown: Some("# Repo audit".into()),
            category: Some(wire::Category::DevTools),
            tags: Some(vec!["audit".into()]),
            sla_seconds: Some(3600),
        },
    );
    check_request("PatchAgentRequest", &wire::PatchAgentRequest::default());
    // The bid: no `agentId` (the token pins the bidder), with and without
    // the promised SLA.
    for sla_seconds in [None, Some(86_400)] {
        check_request(
            "PlaceBidRequest",
            &wire::PlaceBidRequest {
                amount: "2.00".into(),
                proposal: "I will deliver X.".into(),
                sla_seconds,
            },
        );
    }
}

/// The board answer parses minimal (no budget, no creator) and with the
/// optional fields the bidder reads; every enum value of the job's closed
/// sets reads as its text.
#[test]
fn the_board_answer_parses_minimal_and_with_the_optionals_the_bidder_reads() {
    let min: wire::ListBoardJobsResponse = parse(
        "ListBoardJobsResponse",
        instance("ListBoardJobsResponse", Value::Null),
    );
    assert_eq!(min.jobs.len(), 1);
    assert!(min.jobs[0].budget_amount.is_none());
    assert!(min.jobs[0].creator.is_none());
    let job = instance_with(
        "JobView",
        &["budgetAmount", "invitedAgentId", "creator"],
        json!({ "budgetAmount": "5.00", "jobKind": "standard", "status": "filling",
                "minBuilderVerification": "identity_verified", "budgetToken": "USDC" }),
    );
    let v: wire::BoardJobView = parse("JobView", job);
    assert_eq!(v.budget_amount.as_deref(), Some("5.00"));
    // `requiresVerifiable` is required on `Job` and read as is.
    let verifiable = instance("JobView", json!({ "requiresVerifiable": true }));
    let w: wire::BoardJobView = parse("JobView", verifiable.clone());
    assert!(w.requires_verifiable);
    let mut without = verifiable;
    without
        .as_object_mut()
        .unwrap()
        .remove("requiresVerifiable");
    assert!(serde_json::from_value::<wire::BoardJobView>(without).is_err());
    assert_eq!(v.job_kind, "standard");
    assert_eq!(v.status, "filling");
    assert_eq!(v.min_builder_verification, "identity_verified");
    assert!(v.invited_agent_id.is_some());
    assert!(v.creator.is_some());
    for (field, set) in [
        ("jobKind", "JobKind"),
        ("status", "JobStatus"),
        ("minBuilderVerification", "VerificationLevel"),
    ] {
        for value in component_schema(spec(), set)["enum"].as_array().unwrap() {
            let job = instance("JobView", json!({ field: value }));
            let _: wire::BoardJobView = parse("JobView", job);
        }
    }
}

/// The bid list parses with every bid status as text.
#[test]
fn the_bid_list_parses_every_status() {
    for value in component_schema(spec(), "BidStatus")["enum"]
        .as_array()
        .unwrap()
    {
        let bid = instance("Bid", json!({ "status": value }));
        let r: wire::MyBidsResponse = parse(
            "MyBidsResponse",
            instance("MyBidsResponse", json!({ "bids": [bid], "total": 1 })),
        );
        assert_eq!(r.bids[0].status, value.as_str().unwrap());
    }
}

/// Every Rust enum covers exactly its component's closed set.
#[test]
fn every_wire_enum_matches_its_component_set() {
    fn same<T: Serialize + DeserializeOwned>(component: &str, variants: &[T]) {
        let want: Vec<Value> = spec()["components"]["schemas"][component]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{component} is not an enum"))
            .clone();
        let got: Vec<Value> = variants
            .iter()
            .map(|v| serde_json::to_value(v).unwrap())
            .collect();
        assert_eq!(got, want, "{component}");
        for w in &want {
            let _: T = parse(component, w.clone());
        }
    }
    use wire::*;
    same(
        "AssignmentStatus",
        &[
            AssignmentStatus::InProgress,
            AssignmentStatus::Submitted,
            AssignmentStatus::Accepted,
            AssignmentStatus::Disputed,
            AssignmentStatus::DisputeResolved,
            AssignmentStatus::Cancelled,
        ],
    );
    same(
        "JobKind",
        &[JobKind::Standard, JobKind::Instant, JobKind::Competition],
    );
    same(
        "JobStatus",
        &[
            JobStatus::Open,
            JobStatus::Filling,
            JobStatus::InProgress,
            JobStatus::Completed,
            JobStatus::Closed,
        ],
    );
    same(
        "MessageOrigin",
        &[
            MessageOrigin::Direct,
            MessageOrigin::Axi,
            MessageOrigin::Mcp,
            MessageOrigin::Platform,
        ],
    );
    same(
        "ListingStatus",
        &[
            ListingStatus::Draft,
            ListingStatus::Live,
            ListingStatus::Paused,
            ListingStatus::Archived,
        ],
    );
    same(
        "PricingModel",
        &[
            PricingModel::PerCall,
            PricingModel::PerTask,
            PricingModel::Subscription,
            PricingModel::Quote,
        ],
    );
    same(
        "WorkerCancelReason",
        &[WorkerCancelReason::Rejected, WorkerCancelReason::Failed],
    );
    same(
        "Category",
        &[
            Category::Automation,
            Category::Creative,
            Category::CustomerSupport,
            Category::Data,
            Category::DevTools,
            Category::Events,
            Category::Finance,
            Category::LegalCompliance,
            Category::Marketing,
            Category::Operations,
            Category::Research,
            Category::Sales,
            Category::Travel,
            Category::Other,
        ],
    );
    same(
        "Runtime",
        &[Runtime::Http, Runtime::Managed, Runtime::Human],
    );
    same(
        "AgentAvailability",
        &[
            AgentAvailability::Available,
            AgentAvailability::Paused,
            AgentAvailability::Suspended,
            AgentAvailability::UnavailableFunding,
        ],
    );
}

/// Every required property of each parsed component is a field of the
/// wire type: a minimal instance with one required property removed must
/// fail to parse (the type does not silently default it), except where
/// the type deliberately reads a subset (`RequiredSlot`).
#[test]
fn the_parsed_types_require_what_the_components_require() {
    fn each_required<T: DeserializeOwned>(component: &str) {
        let full = instance(component, Value::Null);
        let schema = component_schema(spec(), component);
        for req in schema["required"].as_array().unwrap() {
            let name = req.as_str().unwrap();
            let mut v = full.clone();
            v.as_object_mut().unwrap().remove(name);
            assert!(
                serde_json::from_value::<T>(v).is_err(),
                "{component} parses without its required `{name}`"
            );
        }
    }
    each_required::<wire::MyAssignmentsResponse>("MyAssignmentsResponse");
    each_required::<wire::MyAssignmentItem>("MyAssignmentItem");
    each_required::<wire::JobAssignment>("JobAssignment");
    each_required::<wire::WorkerJobBrief>("WorkerJobBrief");
    each_required::<wire::LatestMessageBrief>("LatestMessageBrief");
    each_required::<wire::AttachmentBrief>("AttachmentBrief");
    each_required::<wire::AgentPublic>("AgentPublic");
    each_required::<wire::OwnerAgentView>("OwnerAgentView");
    each_required::<wire::PricingPlan>("PricingPlan");
    each_required::<wire::ReserveFileResponse>("ReserveFileResponse");
    each_required::<wire::ListBoardJobsResponse>("ListBoardJobsResponse");
    each_required::<wire::MyBidsResponse>("MyBidsResponse");
    each_required::<wire::ListJobAttachmentsResponse>("ListJobAttachmentsResponse");
    each_required::<wire::Bid>("Bid");
}
