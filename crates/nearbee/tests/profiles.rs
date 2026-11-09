//! The shipped profile templates and smoke playbooks (`playbooks/`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn yaml(rel: &str) -> Value {
    let text = std::fs::read_to_string(repo().join(rel)).unwrap();
    yaml_serde::from_str(&text).unwrap()
}

/// Catalog id to owner, from the dated copy of NEAR AI Cloud's model list.
fn catalog() -> BTreeMap<String, String> {
    let text =
        std::fs::read_to_string(repo().join("docs/reference/near-ai-cloud-models.json")).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    v["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["id"].as_str().unwrap().to_owned(),
                m["owned_by"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// Every model a profile names: executor, fallbacks and tiers.
fn models(profile: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |v: &Value| {
        if let Some(m) = v.get("model").and_then(Value::as_str) {
            out.push(m.to_owned());
        }
    };
    push(&profile["executor"]);
    for f in profile["executor"]["fallbacks"]
        .as_array()
        .into_iter()
        .flatten()
    {
        push(f);
    }
    if let Some(tiers) = profile.get("tiers").and_then(Value::as_object) {
        for t in tiers.values() {
            push(t);
        }
    }
    out
}

fn assert_nearai_only(rel: &str) {
    let p = yaml(rel);
    let cat = catalog();
    let names = models(&p);
    assert!(!names.is_empty(), "{rel} names no model");
    for m in names {
        let id = m
            .strip_prefix("nearai/")
            .unwrap_or_else(|| panic!("{rel}: `{m}` is not on the nearai provider"));
        assert_eq!(
            cat.get(id).map(String::as_str),
            Some("nearai"),
            "{rel}: `{id}` is not a nearai-owned catalog model"
        );
    }
    let allow = p["process_env"]["allow"].as_array().unwrap();
    assert!(allow.iter().any(|a| a == "NEARAI_API_KEY"), "{rel}");
}

fn assert_no_secret_values(rel: &str) {
    let text = std::fs::read_to_string(repo().join(rel)).unwrap();
    for word in text.split(|c: char| c.is_whitespace() || ",[]{}:\"'".contains(c)) {
        let w = word.trim();
        assert!(
            !(w.starts_with("sk-") || w.starts_with("apb_") || w.starts_with("aat_")),
            "{rel}: key-shaped value `{w}`"
        );
        let long_random = w.len() >= 32
            && w.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && w.chars().any(|c| c.is_ascii_digit())
            && w.chars().any(|c| c.is_ascii_uppercase())
            && w.chars().any(|c| c.is_ascii_lowercase());
        assert!(!long_random, "{rel}: `{w}` looks like a key");
    }
    for line in text.lines().filter(|l| l.contains("apiKey")) {
        assert!(
            line.contains("\"{env:NEARAI_API_KEY}\""),
            "{rel}: apiKey must come from the environment: {line}"
        );
    }
}

#[test]
fn opencode_provider_lists_the_near_profile_models() {
    let text = std::fs::read_to_string(repo().join("playbooks/workspace/opencode.json")).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let p = &v["provider"]["nearai"];
    assert_eq!(p["options"]["baseURL"], "https://cloud-api.near.ai/v1");
    assert_eq!(p["options"]["apiKey"], "{env:NEARAI_API_KEY}");
    let listed: Vec<&String> = p["models"].as_object().unwrap().keys().collect();
    for m in models(&yaml("playbooks/profiles/nearbee-near.yaml")) {
        let id = m.strip_prefix("nearai/").unwrap().to_owned();
        assert!(listed.contains(&&id), "{id} not in opencode.json");
    }
}

#[test]
fn near_profile_names_only_nearai_models_and_no_key() {
    assert_nearai_only("playbooks/profiles/nearbee-near.yaml");
    assert_nearai_only("playbooks/profiles/nearbee-near-deepseek.yaml");
    for rel in [
        "playbooks/profiles/nearbee-near.yaml",
        "playbooks/profiles/nearbee-near-deepseek.yaml",
        "playbooks/profiles/nearbee-own.yaml",
        "playbooks/profiles/nearbee-near-vision.md",
        "playbooks/workspace/opencode.json",
    ] {
        assert_no_secret_values(rel);
    }
}

#[test]
fn near_profile_template_names_are_stable() {
    assert_eq!(
        yaml("playbooks/profiles/nearbee-near.yaml")["name"],
        "nearbee-near"
    );
    assert_eq!(
        yaml("playbooks/profiles/nearbee-own.yaml")["name"],
        "nearbee-own"
    );
}

#[test]
fn vision_template_is_marked_unsupported_with_the_reason() {
    let text =
        std::fs::read_to_string(repo().join("playbooks/profiles/nearbee-near-vision.md")).unwrap();
    let status = text
        .lines()
        .find(|l| l.to_lowercase().starts_with("status:"))
        .expect("the file has a status line")
        .to_lowercase();
    assert!(status.contains("unsupported"), "{status}");
    assert!(text.to_lowercase().contains("tool call"));
    assert!(
        !repo()
            .join("playbooks/profiles/nearbee-near-vision.yaml")
            .exists()
    );
    let readme = std::fs::read_to_string(repo().join("playbooks/README.md")).unwrap();
    let row = readme
        .lines()
        .find(|l| l.starts_with('|') && l.contains("nearbee-near-vision"))
        .expect("the README table names the vision template")
        .to_lowercase();
    assert!(
        row.contains("unsupported") && row.contains("tool call"),
        "{row}"
    );
}

#[test]
fn smoke_copies_differ_only_in_id_name_and_profile() {
    let mut base: Option<Value> = None;
    for (id, profile) in [
        ("smoke-own", "nearbee-own"),
        ("smoke-qwen", "nearbee-near"),
        ("smoke-deepseek", "nearbee-near-deepseek"),
    ] {
        let mut p = yaml(&format!("playbooks/smoke/{id}.yaml"));
        assert_eq!(p["id"], id);
        assert_eq!(p["defaults"]["profile"], profile);
        assert!(
            repo()
                .join(format!("playbooks/profiles/{profile}.yaml"))
                .exists()
        );
        let params = p["params"].as_array().unwrap();
        for name in ["hire_id", "execution_id"] {
            let param = params.iter().find(|x| x["name"] == name).unwrap();
            assert_eq!(param["search"], true, "{id}: {name}");
        }
        let obj = p.as_object_mut().unwrap();
        obj.remove("id");
        obj.remove("name");
        obj["defaults"].as_object_mut().unwrap().remove("profile");
        match &base {
            None => base = Some(p),
            Some(b) => assert_eq!(b, &p, "{id} differs from smoke-own"),
        }
    }
}

/// Every key path in `v` whose last key is one of `names`.
fn keys_named(v: &Value, names: &[&str], at: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let here = format!("{at}.{k}");
                if names.contains(&k.as_str()) {
                    out.push(here.clone());
                }
                keys_named(x, names, &here, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                keys_named(x, names, &format!("{at}[{i}]"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn smoke_playbooks_name_executors_only_through_profiles() {
    for id in ["smoke-own", "smoke-qwen", "smoke-deepseek"] {
        let p = yaml(&format!("playbooks/smoke/{id}.yaml"));
        let mut found = Vec::new();
        keys_named(
            &p,
            &["model", "agent", "executor", "profile", "fallbacks", "tier"],
            "",
            &mut found,
        );
        assert_eq!(
            found,
            [".defaults.profile"],
            "{id}: executors only through defaults.profile"
        );
    }
}
