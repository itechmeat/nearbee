//! Agent B's playbook and its listing (task 6.1 of the change
//! `doc-digest-and-paid-demo`; design 7, spec `doc-digest`).

use std::path::{Path, PathBuf};

use nearbee_apb::install::{self, Script};
use nearbee_core::catalog::Catalog;
use nearbee_core::listing::{self, Tier};
use nearbee_core::tiers::{self, ExecutionFacts, Known, NodeFact, ProviderRoute};

const STEPS: [&str; 4] = ["pages", "extract", "classify", "compose"];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playbooks/doc-digest")
}

fn scripts() -> Vec<Script> {
    STEPS
        .iter()
        .map(|n| Script {
            name: format!("{n}.sh"),
            bytes: std::fs::read(dir().join("scripts").join(format!("{n}.sh"))).unwrap(),
            executable: true,
        })
        .collect()
}

fn load() -> listing::Listing {
    let path = dir().join("listing.doc-digest-private.yaml");
    listing::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap()
}

fn yaml(text: &str) -> serde_json::Value {
    yaml_serde::from_str(text).unwrap()
}

#[test]
fn the_listing_loads_with_its_direct_and_signed_models() {
    let l = load();
    assert_eq!(l.handle, "doc-digest-private");
    assert_eq!(l.tier, Tier::Attested);
    assert_eq!(l.profile, None);
    assert_eq!(l.price_usd, "1.00");
    assert_eq!(l.sla_seconds, 86400);
    assert_eq!(l.category, "data");
    assert_eq!(
        l.direct_models,
        [
            "Qwen/Qwen3-VL-30B-A3B-Instruct",
            "openai/privacy-filter",
            "deepseek-ai/DeepSeek-V4.1-Flash-1017"
        ]
    );
    assert_eq!(l.signed_models, ["deepseek-ai/DeepSeek-V4.1-Flash-1017"]);
    let limits = l.limits.clone().unwrap();
    assert_eq!(limits.max_attachments, Some(10));
    assert_eq!(limits.max_attachment_bytes, Some(26_214_400));
    let types = limits.file_types.unwrap();
    assert_eq!(types, ["pdf", "png", "jpg", "jpeg"]);
    // A `.jpg` attachment passes the listing's file types; a `.docx` does
    // not.
    for name in ["scan.jpg", "scan.JPEG", "doc.pdf", "page.png"] {
        assert!(
            nearbee_core::intake::file_type_allowed(name, &types),
            "{name}"
        );
    }
    assert!(!nearbee_core::intake::file_type_allowed("doc.docx", &types));
    let text = format!("{} {}", l.description, l.bio_markdown);
    assert!(!text.contains('!'), "{text}");
}

/// A job row with attachments of the given declared sizes.
fn job(sizes: &[i64]) -> nearbee_core::market::Row {
    let files: Vec<serde_json::Value> = sizes
        .iter()
        .enumerate()
        .map(|(i, size)| {
            serde_json::json!({"id": format!("f{i}"), "filename": format!("scan-{i}.pdf"),
                "contentType": "application/pdf", "byteSize": size, "sha256": "ab",
                "downloadUrl": format!("/v1/files/f{i}")})
        })
        .collect();
    serde_json::from_value(serde_json::json!({"id": "a1", "status": "in_progress",
        "title": "Digest", "description": "Digest the attached invoices.",
        "jobAttachments": files}))
    .unwrap()
}

#[test]
fn intake_declines_the_attachments_that_the_pages_step_would_refuse() {
    // `max_attachment_bytes` bounds the sum of the declared sizes, so the
    // listing's bound equals the 25 MiB total that `digest pages` checks.
    let limits = load().limits.unwrap();
    assert_eq!(
        limits.max_attachment_bytes,
        Some(nearbee::digest::MAX_TOTAL_BYTES)
    );
    assert_eq!(limits.max_attachments, Some(nearbee::digest::MAX_FILES));
    let mib = 1024 * 1024;
    // Two 15 MiB scans: within 10 files, over 25 MiB in total.
    assert!(matches!(
        nearbee_core::intake::hard_limits(&job(&[15 * mib, 15 * mib]), Some(&limits)),
        nearbee_core::intake::Outcome::Declined { ref buyer, .. }
            if buyer.contains("max_attachment_bytes 26214400")
    ));
    // Ten files of 2.5 MiB fill the total exactly and pass.
    assert_eq!(
        nearbee_core::intake::hard_limits(&job(&[2_621_440; 10]), Some(&limits)),
        nearbee_core::intake::Outcome::passed()
    );
    // An eleventh file is declined.
    assert!(matches!(
        nearbee_core::intake::hard_limits(&job(&[1; 11]), Some(&limits)),
        nearbee_core::intake::Outcome::Declined { ref buyer, .. }
            if buyer.contains("max_attachments 10")
    ));
}

#[test]
fn the_playbook_runs_four_nearbee_commands_and_no_agent_step() {
    let v = yaml(&std::fs::read_to_string(dir().join("playbook.yaml")).unwrap());
    assert_eq!(v["id"], "doc-digest");
    assert!(
        v.get("defaults").is_none(),
        "no default profile: no agent step"
    );
    let nodes = v["nodes"].as_array().unwrap();
    let ids: Vec<&str> = nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        ["start", "pages", "extract", "classify", "compose", "done"]
    );
    for step in STEPS {
        let node = nodes.iter().find(|n| n["id"] == step).unwrap();
        assert_eq!(node["type"], "script");
        let script = format!("scripts/{step}.sh");
        assert_eq!(node["script"], script.as_str());
        let text = std::fs::read_to_string(dir().join(&script)).unwrap();
        assert!(
            text.contains(&format!("exec \"${{NEARBEE_BIN:-nearbee}}\" digest {step}")),
            "{text}"
        );
    }
    let compose = nodes.iter().find(|n| n["id"] == "compose").unwrap();
    assert_eq!(compose["expected_duration"], "6m");
    let params: Vec<&str> = v["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        params,
        [
            "brief",
            "feedback",
            "inputs",
            "hire_id",
            "execution_id",
            "direct_models"
        ]
    );
    let direct = v["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "direct_models")
        .unwrap();
    assert_eq!(direct["default"], "[]");
}

/// The listing materialises, and the attested check passes on the facts
/// such a playbook gives with the listing's direct models.
#[test]
fn the_listing_materialises_and_passes_the_attested_check() {
    let l = load();
    let source = std::fs::read_to_string(dir().join("playbook.yaml")).unwrap();
    let m = install::materialise(&source, &l.handle, l.profile.as_deref(), &scripts()).unwrap();
    let v = yaml(&m);
    assert_eq!(v["id"], "doc-digest-private");
    assert_eq!(v["supervisor"]["policy"]["trial_candidates"], "off");
    let nodes: Vec<NodeFact> = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| NodeFact {
            id: n["id"].as_str().unwrap().into(),
            kind: n["type"].as_str().unwrap().into(),
            interactive: false,
            profile: None,
            connectors: Vec::new(),
            playbook_ref: None,
        })
        .collect();
    let facts = ExecutionFacts {
        playbook_id: l.handle.clone(),
        default_profile: None,
        nodes,
        profiles: Vec::new(),
        routes: Known::Yes(vec![ProviderRoute {
            name: "nearai".into(),
            base_url: tiers::NEARAI_BASE_URL.into(),
            api_key_ref: tiers::NEARAI_API_KEY_REF.into(),
            model_overrides: Vec::new(),
        }]),
        decisions_enabled: Known::Yes(false),
        compaction_enabled: Known::Yes(false),
        direct_models: Vec::new(),
        signed_models: Vec::new(),
    }
    .with_direct_models(&l.direct_models, &l.signed_models);
    let catalog = Catalog::embedded().unwrap();
    assert_eq!(tiers::check(Tier::Attested, &facts, catalog), Ok(()));
    assert_eq!(tiers::evidence_models(&facts).len(), 3);
    let text = nearbee_core::statement::render(Tier::Attested, &facts, 7);
    assert!(text.contains("No agent step runs a model."), "{text}");
    assert!(text.contains("The signed step"), "{text}");
}
