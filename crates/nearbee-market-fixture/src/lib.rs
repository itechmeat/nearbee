//! Schema-shaped market bodies for tests: the one shared fixture builder
//! behind the contract test (`nearbee-market/tests/contract.rs`) and every
//! wiremock test that mocks the market, in `nearbee-market` and `nearbee`.
//! A test-support crate: only dev-dependencies name it, so nothing of it
//! ships in the production crates.
//!
//! A body is built from the vendored OpenAPI document
//! (`docs/reference/market-openapi.json`): every required property of the
//! component gets a value of its schema type (recursively, through `$ref`,
//! `allOf`, `oneOf`), and the caller's overrides are merged in by property
//! name. A mock therefore always carries what the API promises, never only
//! what the client happens to read. Not used by the client itself.

use std::sync::OnceLock;

use serde_json::{Map, Value, json};

/// The vendored OpenAPI document, read once.
pub fn spec() -> &'static Value {
    static SPEC: OnceLock<Value> = OnceLock::new();
    SPEC.get_or_init(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/reference/market-openapi.json"
        );
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read the market OpenAPI at {path}: {e}"));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("the market OpenAPI at {path} is not JSON: {e}"))
    })
}

/// A minimal instance of `component` (its required properties), with
/// `overrides` merged in: an object merges by property name, recursively,
/// and any other value replaces the built one. An override may also name
/// an optional property; it is then built from its schema and merged.
pub fn instance(component: &str, overrides: Value) -> Value {
    schema_instance(spec(), component, &overrides)
}

/// Like [`instance`], with the named optional properties built as well
/// (non-null shapes), so a test can exercise the optional fields the
/// client reads.
pub fn instance_with(component: &str, optional: &[&str], overrides: Value) -> Value {
    let root = spec();
    let node = deref(&component_ref(component), root);
    let mut v = schema_instance(root, component, &overrides);
    let props = node.get("properties").and_then(Value::as_object);
    if let (Some(obj), Some(props)) = (v.as_object_mut(), props) {
        for name in optional {
            let child = props
                .get(*name)
                .unwrap_or_else(|| panic!("{component} has no property {name}"));
            if !obj.contains_key(*name) {
                obj.insert((*name).to_owned(), build(child, root, &Value::Null));
            }
        }
    }
    v
}

/// [`instance`] against an explicit document.
pub fn schema_instance(spec: &Value, component: &str, overrides: &Value) -> Value {
    build(&component_ref(component), spec, overrides)
}

/// The component's schema node with `$ref`, `allOf` and `oneOf` resolved
/// (`allOf` branches merged; the first non-null `oneOf`/`anyOf` branch).
pub fn component_schema(spec: &Value, component: &str) -> Value {
    deref(&component_ref(component), spec)
}

fn component_ref(component: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{component}") })
}

// ---------------------------------------------------------------------------
// Market bodies the tests mock
// ---------------------------------------------------------------------------

/// One `MyAssignmentItem` row: `assignment.assignmentId` and
/// `assignment.status` set, the rest of `assignment` and `job` merged
/// from the given overrides.
pub fn assignment_item(id: &str, status: &str, assignment: Value, job: Value) -> Value {
    let mut a = json!({ "assignmentId": id, "status": status });
    merge(&mut a, assignment);
    let mut item = json!({ "assignment": a });
    if !job.is_null() {
        item["job"] = job;
    }
    instance("MyAssignmentItem", item)
}

/// The `MyAssignmentsResponse` envelope around the given rows.
pub fn assignments(items: Vec<Value>) -> Value {
    instance("MyAssignmentsResponse", json!({ "assignments": items }))
}

/// The public agent view (`AgentPublic`), as a non-owner reads it.
pub fn agent_public(agent_id: &str, handle: &str, listing_status: &str) -> Value {
    instance(
        "AgentPublic",
        json!({ "agent_id": agent_id, "handle": handle, "listing_status": listing_status }),
    )
}

/// The owner's agent view (`OwnerAgentView`), the PATCH answer.
pub fn owner_agent_view(agent_id: &str, handle: &str, listing_status: &str) -> Value {
    instance(
        "OwnerAgentView",
        json!({ "agent_id": agent_id, "handle": handle, "listing_status": listing_status }),
    )
}

/// One `PricingPlan`.
pub fn pricing_plan(
    id: &str,
    agent_id: &str,
    price: &str,
    sla_seconds: Option<u64>,
    is_default: bool,
) -> Value {
    let mut o = json!({
        "pricing_plan_id": id,
        "agent_id": agent_id,
        "model": "per_call",
        "price_amount": price,
        "is_default": is_default,
        "listing_status": "live",
    });
    if let Some(s) = sla_seconds {
        o["sla_seconds"] = json!(s);
    }
    instance("PricingPlan", o)
}

/// The `ReserveFileResponse` of a linked reserve.
pub fn reserve_file_response(file_id: &str, upload_url: &str, file_url: &str) -> Value {
    instance(
        "ReserveFileResponse",
        json!({
            "fileId": file_id,
            "uploadUrl": upload_url,
            "directUploadUrl": upload_url,
            "fileUrl": file_url,
        }),
    )
}

/// The documented success body of a write the client does not read
/// (`StartAssignmentResponse`, `ThreadMessage`, `WorkerProgressReport`,
/// `JobAssignment`, `CompleteFileResponse`, `PricingPlan`).
pub fn write_answer(component: &str) -> Value {
    instance(component, Value::Null)
}

/// A market refusal body (`ApiErrorBody`-like `{error, message}`).
pub fn refusal(code: &str, message: &str) -> Value {
    json!({ "error": code, "message": message })
}

fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v),
                    _ => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) if !o.is_null() => *b = o,
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// The builder
// ---------------------------------------------------------------------------

fn resolve<'a>(root: &'a Value, r: &str) -> &'a Value {
    let node = r.strip_prefix("#/").unwrap_or(r);
    let mut cur = root;
    for part in node.split('/') {
        cur = &cur[part];
    }
    cur
}

fn is_null_schema(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("null")
}

/// Follows `$ref`, merges `allOf` branches and takes the first non-null
/// `oneOf`/`anyOf` branch, down to one schema node.
fn deref(node: &Value, root: &Value) -> Value {
    if let Some(r) = node.get("$ref").and_then(Value::as_str) {
        return deref(resolve(root, r), root);
    }
    if let Some(arr) = node.get("allOf").and_then(Value::as_array) {
        let mut props = Map::new();
        let mut required = Vec::new();
        for branch in arr {
            let b = deref(branch, root);
            if let Some(p) = b.get("properties").and_then(Value::as_object) {
                for (k, v) in p {
                    props.insert(k.clone(), v.clone());
                }
            }
            if let Some(r) = b.get("required").and_then(Value::as_array) {
                required.extend(r.iter().cloned());
            }
        }
        return json!({ "type": "object", "properties": props, "required": required });
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(arr) = node.get(key).and_then(Value::as_array) {
            let pick = arr
                .iter()
                .find(|b| !is_null_schema(b))
                .or_else(|| arr.first());
            if let Some(b) = pick {
                return deref(b, root);
            }
        }
    }
    node.clone()
}

/// The schema's type, the non-null one of `type: [.., "null"]`.
fn schema_type(node: &Value) -> Option<String> {
    match node.get("type") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .map(str::to_owned),
        _ => None,
    }
}

fn build(node: &Value, root: &Value, over: &Value) -> Value {
    let node = deref(node, root);
    let ty = schema_type(&node).unwrap_or_else(|| {
        if node.get("properties").is_some() || node.get("required").is_some() {
            "object".into()
        } else {
            String::new()
        }
    });
    // A non-object override, or any override of a non-object schema,
    // replaces the built value.
    if !over.is_null() && !(ty == "object" && over.is_object()) {
        return over.clone();
    }
    if let Some(first) = node
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
    {
        return first.clone();
    }
    match ty.as_str() {
        "object" => {
            let mut out = Map::new();
            let props = node.get("properties").and_then(Value::as_object);
            let over_object = over.as_object();
            for k in node
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = k.as_str().unwrap_or_default();
                let child = props
                    .and_then(|p| p.get(name))
                    .cloned()
                    .unwrap_or(Value::Null);
                let child_over = over_object
                    .and_then(|o| o.get(name))
                    .cloned()
                    .unwrap_or(Value::Null);
                out.insert(name.to_owned(), build(&child, root, &child_over));
            }
            // Optional properties appear only when an override names them.
            if let Some(over) = over_object {
                for (name, child_over) in over {
                    if out.contains_key(name) {
                        continue;
                    }
                    match props.and_then(|p| p.get(name)) {
                        Some(child) => {
                            out.insert(name.clone(), build(child, root, child_over));
                        }
                        None => panic!("override names `{name}`, which the schema does not have"),
                    }
                }
            }
            Value::Object(out)
        }
        "array" => match node.get("items") {
            Some(items) => Value::Array(vec![build(items, root, &Value::Null)]),
            None => Value::Array(Vec::new()),
        },
        "string" => match node.get("format").and_then(Value::as_str) {
            Some("date-time") => json!("2026-11-09T12:00:00Z"),
            Some("uuid") => json!("00000000-0000-4000-8000-000000000000"),
            _ => json!("nearbee"),
        },
        "integer" | "number" => json!(node.get("minimum").and_then(Value::as_i64).unwrap_or(0)),
        "boolean" => json!(false),
        _ => Value::Null,
    }
}
