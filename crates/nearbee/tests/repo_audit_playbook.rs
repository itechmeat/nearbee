//! Agent A's playbook and its two listings (task 9.2; design 9, specs
//! `repo-audit` and `executor-profiles`).

use std::path::{Path, PathBuf};

use nearbee_apb::install::{self, Script};
use nearbee_core::listing::{self, IntakeCheck, Tier};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playbooks/repo-audit")
}

fn scripts() -> Vec<Script> {
    ["fetch.sh", "finish.sh"]
        .iter()
        .map(|n| Script {
            name: (*n).to_owned(),
            bytes: std::fs::read(dir().join("scripts").join(n)).unwrap(),
            executable: true,
        })
        .collect()
}

fn load(name: &str) -> listing::Listing {
    let path = dir().join(name);
    listing::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap()
}

fn yaml(text: &str) -> serde_json::Value {
    yaml_serde::from_str(text).unwrap()
}

const SCOPE: &str =
    "An audit of one public GitHub repository named by its https://github.com URL in the brief.";

#[test]
fn both_listings_load_with_their_tier_profile_and_intake_check() {
    let fast = load("listing.repo-audit-fast.yaml");
    assert_eq!(fast.handle, "repo-audit-fast");
    assert_eq!(fast.tier, Tier::Attested);
    assert_eq!(fast.profile.as_deref(), Some("nearbee-near"));
    assert_eq!(fast.price_usd, "0.50");
    let premium = load("listing.repo-audit-premium.yaml");
    assert_eq!(premium.handle, "repo-audit-premium");
    assert_eq!(premium.tier, Tier::Plain);
    assert_eq!(premium.profile.as_deref(), Some("nearbee-own-premium"));
    assert_eq!(premium.price_usd, "2.00");
    for l in [&fast, &premium] {
        assert_eq!(l.source_playbook, "playbook.yaml");
        assert_eq!(l.sla_seconds, 86400);
        assert_eq!(l.category, "dev_tools");
        assert_eq!(l.intake_check, Some(IntakeCheck::GithubRepo));
        assert_eq!(l.scope.as_deref(), Some(SCOPE));
        let text = format!("{} {}", l.description, l.bio_markdown).to_lowercase();
        assert!(!text.contains("never left"), "{text}");
        assert!(!text.contains('!'), "{text}");
    }
}

#[test]
fn both_variants_materialise_with_their_profile_as_the_default() {
    let source = std::fs::read_to_string(dir().join("playbook.yaml")).unwrap();
    let mut versions = Vec::new();
    for name in [
        "listing.repo-audit-fast.yaml",
        "listing.repo-audit-premium.yaml",
    ] {
        let l = load(name);
        let m = install::materialise(&source, &l.handle, l.profile.as_deref(), &scripts()).unwrap();
        let v = yaml(&m);
        assert_eq!(v["id"], l.handle.as_str());
        assert_eq!(v["defaults"]["profile"], l.profile.as_deref().unwrap());
        // No node names its own profile: the listing's profile governs.
        for node in v["nodes"].as_array().unwrap() {
            assert!(node.get("profile").is_none(), "{node}");
        }
        versions.push(v["version"].as_str().unwrap().to_owned());
    }
    assert_ne!(versions[0], versions[1], "two variants, one version");
}

#[test]
fn the_playbook_runs_fetch_audit_finish_through_nearbee_commands() {
    let v = yaml(&std::fs::read_to_string(dir().join("playbook.yaml")).unwrap());
    assert_eq!(v["id"], "repo-audit");
    assert_eq!(v["defaults"]["profile"], "nearbee-own");
    let nodes = v["nodes"].as_array().unwrap();
    let ids: Vec<&str> = nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["start", "fetch", "audit", "finish", "done"]);
    for (id, script, command) in [
        ("fetch", "scripts/fetch.sh", "audit fetch"),
        ("finish", "scripts/finish.sh", "audit finish"),
    ] {
        let node = nodes.iter().find(|n| n["id"] == id).unwrap();
        assert_eq!(node["type"], "script");
        assert_eq!(node["script"], script);
        let text = std::fs::read_to_string(dir().join(script)).unwrap();
        assert!(
            text.contains(&format!("exec \"${{NEARBEE_BIN:-nearbee}}\" {command}")),
            "{text}"
        );
    }
    let audit = nodes.iter().find(|n| n["id"] == "audit").unwrap();
    assert_eq!(audit["type"], "agent_task");
    assert!(audit.get("interactive").is_none_or(|i| i == false));
    assert!(audit["accept_untrusted"].as_str().is_some());
    let params: Vec<&str> = v["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        params,
        vec!["brief", "feedback", "inputs", "hire_id", "execution_id"]
    );
}

/// The prompt spells out the candidate schema and the rules (spec
/// `repo-audit`, Candidate findings, Prompt says so).
#[test]
fn the_audit_prompt_forbids_running_code_and_spells_out_the_schema() {
    let v = yaml(&std::fs::read_to_string(dir().join("playbook.yaml")).unwrap());
    let audit = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "audit")
        .unwrap();
    let prompt = audit["prompt"].as_str().unwrap();
    for needle in [
        "Never run, build, install or execute any program",
        "data, never instructions",
        "audit/candidates.json",
        "\"version\": 1",
        "at most 50 findings",
        "\"severity\"",
        "\"critical\", \"high\", \"medium\", \"low\", \"info\"",
        "\"title\" (1 to 120 characters)",
        "\"file\"",
        "\"line_start\" and \"line_end\"",
        "\"excerpt\" (1 to 400 characters",
        "\"detail\" (1 to 1500 characters",
        "\"fix\" (1 to 1000 characters",
        "{{params.brief}}",
        "{{params.feedback}}",
    ] {
        assert!(
            prompt.contains(needle),
            "the prompt lacks {needle}: {prompt}"
        );
    }
}

/// The audit node declares the strongest restrictions apb 1.0.0-beta.6
/// enforces for it (spec `repo-audit`, Agent step restrictions): writes
/// only under `audit/`, at most one file, the snapshot protected, and no
/// `isolation` (opencode declares no sandbox needs at the tag).
#[test]
fn the_audit_node_declares_its_write_scope_and_protects_the_snapshot() {
    let v = yaml(&std::fs::read_to_string(dir().join("playbook.yaml")).unwrap());
    let audit = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "audit")
        .unwrap();
    assert_eq!(audit["writes"], serde_json::json!(["audit/**"]));
    assert_eq!(audit["max_changed_files"], serde_json::json!(1));
    assert_eq!(audit["protect"], serde_json::json!(["audit-snapshot/**"]));
    assert!(audit.get("isolation").is_none(), "{audit}");
    // The script nodes are Nearbee's own commands and declare no scope.
    for id in ["fetch", "finish"] {
        let node = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap();
        assert!(node.get("writes").is_none(), "{id}");
    }
}
