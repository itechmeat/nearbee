//! CLI tests of `nearbee list` against a wiremock market and a fake apb
//! CLI: the market update scenarios. Publishing never touches
//! `listing_status`.

use nearbee::publish::{PublishEnv, publish};
use nearbee::{Cli, Command, Env, ListArgs};
use nearbee_core::credentials::{AgentCredentials, Store};
use nearbee_core::secret::Secret;
use nearbee_market_fixture as fx;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "aat_0123456789abcdef0123456789abcdef";
const HANDLE: &str = "repo-audit-fast";
const AGENT_ID: &str = "ag_123";

/// A fake apb CLI that imports and approves anything, with a fixed
/// installed version.
const FAKE: &str = r#"#!/bin/sh
STATE="${APB_CONFIG_DIR:?}"
bundle="$2"
id="$(sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' "$bundle")"
ver="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$bundle")"
case "$1" in
  import)
    if [ "$3" = "--accept" ]; then touch "$STATE/holds"; printf '%s' "$ver" > "$STATE/installed-version.txt"; echo imported; exit 0; fi
    if [ -f "$STATE/holds" ]; then printf '%s' "$ver" > "$STATE/installed-version.txt"; echo "bundle $id @ $ver: unchanged: the current version already holds this bundle"; exit 0; fi
    echo "bundle $id @ $ver is new"
    echo "the imported version carries scripts: it runs only after an approval that names its digest"
    echo "--accept sha256:deadbeef"
    exit 3
    ;;
  trust)
    echo approved; exit 0 ;;
  export)
    version="$(cat "$STATE/installed-version.txt" 2>/dev/null || echo 1.0.0)"
    playbook="$(sed -n 's/.*"playbook": *"\(.*\)"}/\1/p' "$STATE/readback.json" 2>/dev/null)"
    printf '{"apb_bundle":2,"id":"%s","version":"%s","playbook":"%s"}\n' "$2" "$version" "$playbook" > "$3"
    exit 0 ;;
  *) echo "fake: $*"; exit 2 ;;
esac
"#;

struct Setup {
    dir: tempfile::TempDir,
    server: MockServer,
    env: PublishEnv,
    store: Store,
}

impl Setup {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("holds"), "").unwrap();
        std::fs::write(
            state.join("readback.json"),
            json!({
                "apb_bundle": 2,
                "id": HANDLE,
                "playbook": "id: repo-audit-fast\nname: x\nsupervisor:\n  policy:\n    trial_candidates: off\n"
            })
            .to_string(),
        )
        .unwrap();
        let bin = dir.path().join("fake-apb.sh");
        std::fs::write(&bin, FAKE).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = PublishEnv {
            market_url: server.uri(),
            apb_bin: bin,
            apb_config_dir: state.clone(),
            apb_workspace: dir.path().join("ws"),
        };
        std::fs::create_dir_all(&env.apb_workspace).unwrap();
        let store_path = dir.path().join("nearbee").join("credentials");
        let store = Store::at(store_path);
        store
            .put(
                HANDLE,
                AgentCredentials {
                    agent_id: AGENT_ID.into(),
                    token: Secret::new(TOKEN),
                },
            )
            .unwrap();
        // The source playbook lives beside the listing file, inside its
        // directory tree.
        let src = "repo-audit-fast.yaml";
        std::fs::write(
            dir.path().join(src),
            std::fs::read_to_string(playbook_path()).unwrap(),
        )
        .unwrap();
        let listing_file = dir.path().join("listing.yaml");
        std::fs::write(
            &listing_file,
            format!(
                "handle: {HANDLE}\nname: \"Repo audit (fast)\"\nsource_playbook: {src}\nprice_usd: \"0.50\"\nsla_seconds: 86400\ncategory: dev_tools\ntags: [\"nearbee\", \"example\"]\ndescription: \"Audits a repository.\"\nbio_markdown: \"Nearbee example.\"\n"
            ),
        )
        .unwrap();
        Self {
            dir,
            server,
            env,
            store,
        }
    }

    fn listing(&self) -> PathBuf {
        self.dir.path().join("listing.yaml")
    }

    fn state_dir(&self) -> PathBuf {
        self.dir.path().join("state-nearbee")
    }

    /// The fake apb's state dir, where its export read back comes from.
    fn apb_state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn secrets(&self) -> Vec<Secret> {
        vec![]
    }
}

fn playbook_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../playbooks/first-hire/repo-audit-fast.yaml")
        .display()
        .to_string()
}

/// The plan list with one default plan at the given price and SLA.
fn old_default_plan(price: &str, sla: u64) -> Value {
    json!([fx::pricing_plan(
        "plan_old",
        AGENT_ID,
        price,
        Some(sla),
        true
    )])
}

async fn mount_market(server: &MockServer, plans: Value) {
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/agents/{AGENT_ID}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(fx::owner_agent_view(AGENT_ID, HANDLE, "draft")),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{AGENT_ID}/pricing-plans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(plans))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/agents/{AGENT_ID}/pricing-plans")))
        .respond_with(ResponseTemplate::new(201).set_body_json(fx::pricing_plan(
            "plan_new",
            AGENT_ID,
            "0.50",
            Some(86400),
            false,
        )))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pricing-plans/plan_new/make-default"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::pricing_plan(
            "plan_new",
            AGENT_ID,
            "0.50",
            Some(86400),
            true,
        )))
        .mount(server)
        .await;
}

#[tokio::test]
async fn publish_patches_and_creates_a_plan_when_price_changed() {
    let s = Setup::new().await;
    let plans = old_default_plan("1.00", 3600);
    mount_market(&s.server, plans).await;
    let r = publish(&s.listing(), &s.env, &s.store, &s.state_dir(), s.secrets())
        .await
        .unwrap();
    assert_eq!(r.handle, HANDLE);
    assert_eq!(r.agent_id, AGENT_ID);
    // The installed version read back equals the derived version of the
    // materialised playbook.
    assert!(
        r.playbook_version.starts_with("1.0."),
        "{}",
        r.playbook_version
    );
    assert_eq!(r.playbook_version.len(), "1.0.".len() + 8);
    assert_eq!(r.price_usd, "0.50");
    // The resolved listing is on disk.
    assert!(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json"))
            .exists()
    );
    // The patch never carries listing_status.
    let bodies = s.server.received_requests().await.unwrap();
    let patch = bodies
        .iter()
        .find(|req| req.url.path() == format!("/v1/agents/{AGENT_ID}"))
        .unwrap();
    let body: Value = serde_json::from_slice(&patch.body).unwrap();
    assert!(body.get("listing_status").is_none(), "{body}");
    assert_eq!(body["category"], "dev_tools");
    assert_eq!(body["sla_seconds"], 86400);
    // A plan was created and made default.
    let plans_path = format!("/v1/agents/{AGENT_ID}/pricing-plans");
    assert!(
        bodies
            .iter()
            .any(|req| req.url.path() == plans_path.as_str() && req.method.as_str() == "POST")
    );
    assert!(
        bodies
            .iter()
            .any(|req| req.url.path() == "/v1/pricing-plans/plan_new/make-default")
    );
}

#[tokio::test]
async fn an_unchanged_price_creates_no_new_plan() {
    let s = Setup::new().await;
    let plans = old_default_plan("0.50", 86400);
    mount_market(&s.server, plans).await;
    let r = publish(&s.listing(), &s.env, &s.store, &s.state_dir(), s.secrets())
        .await
        .unwrap();
    assert_eq!(r.handle, HANDLE);
    let bodies = s.server.received_requests().await.unwrap();
    let plans_path = format!("/v1/agents/{AGENT_ID}/pricing-plans");
    assert!(
        !bodies
            .iter()
            .any(|req| req.url.path() == plans_path.as_str() && req.method.as_str() == "POST"),
        "a plan was created although the default already matched"
    );
    assert!(
        !bodies
            .iter()
            .any(|req| req.url.path().ends_with("make-default")),
        "make-default was called although the default already matched"
    );
}

#[tokio::test]
async fn a_pricing_plan_409_names_the_cap_and_changes_nothing() {
    let s = Setup::new().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/agents/{AGENT_ID}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(fx::owner_agent_view(AGENT_ID, HANDLE, "draft")),
        )
        .mount(&s.server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{AGENT_ID}/pricing-plans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(old_default_plan("1.00", 3600)))
        .mount(&s.server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/agents/{AGENT_ID}/pricing-plans")))
        .respond_with(ResponseTemplate::new(409).set_body_json(fx::refusal(
            "pricing_plan_cap",
            "maximum number of plans reached",
        )))
        .mount(&s.server)
        .await;
    let err = publish(&s.listing(), &s.env, &s.store, &s.state_dir(), s.secrets())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("pricing_plan_cap"), "{msg}");
    assert!(msg.contains("remove an unused plan"), "{msg}");
    // No plan was deleted or patched and no make-default happened.
    let bodies = s.server.received_requests().await.unwrap();
    assert!(
        !bodies.iter().any(|req| {
            (req.url.path().contains("/pricing-plans"))
                && matches!(req.method.as_str(), "DELETE" | "PATCH" | "PUT")
        }),
        "a plan was deleted or patched"
    );
    let bodies = s.server.received_requests().await.unwrap();
    assert!(
        !bodies
            .iter()
            .any(|req| req.url.path().ends_with("make-default")),
        "make-default was called after the cap refusal"
    );
}

#[tokio::test]
async fn publishing_stops_before_any_market_call_when_the_playbook_cannot_be_installed() {
    let s = Setup::new().await;
    // A source that sets trial_candidates to another value: the install
    // must refuse before the first market call.
    std::fs::write(
        s.dir.path().join("bad.yaml"),
        "supervisor:\n  policy:\n    trial_candidates: auto\nid: repo-audit-fast\nversion: 1.0.0\nname: x\nnodes:\n",
    )
    .unwrap();
    std::fs::write(
        s.dir.path().join("listing-bad.yaml"),
        format!(
            "handle: {HANDLE}\nname: x\nsource_playbook: bad.yaml\nprice_usd: \"0.50\"\nsla_seconds: 86400\ncategory: dev_tools\ntags: [\"t\"]\ndescription: \"d\"\nbio_markdown: \"b\"\n"
        ),
    )
    .unwrap();
    let err = publish(
        &s.dir.path().join("listing-bad.yaml"),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("trial_candidates"), "{err}");
    let bodies = s.server.received_requests().await.unwrap();
    assert!(
        bodies.is_empty(),
        "a market call was made: {}",
        bodies.len()
    );

    // An installed current version that reads back without the trial
    // setting refuses before any market call too.
    std::fs::write(
        s.apb_state().join("readback.json"),
        json!({
            "apb_bundle": 2,
            "id": HANDLE,
            "playbook": "id: repo-audit-fast\nname: x\n"
        })
        .to_string(),
    )
    .unwrap();
    let err = publish(&s.listing(), &s.env, &s.store, &s.state_dir(), s.secrets())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("trial_candidates"), "{err}");
    let bodies = s.server.received_requests().await.unwrap();
    assert!(
        bodies.is_empty(),
        "a market call was made after a bad read back: {}",
        bodies.len()
    );
    // Restore the good read back for the later tests.
    std::fs::write(
        s.apb_state().join("readback.json"),
        json!({
            "apb_bundle": 2,
            "id": HANDLE,
            "playbook": "id: repo-audit-fast\nname: x\nsupervisor:\n  policy:\n    trial_candidates: off\n"
        })
        .to_string(),
    )
    .unwrap();
}

/// The CLI command `nearbee list` drives the same publish through
/// the config file and environment: it prints the summary and exits 0.
#[tokio::test]
async fn the_cli_publish_list_command_prints_the_summary() {
    let s = Setup::new().await;
    let plans = old_default_plan("1.00", 3600);
    mount_market(&s.server, plans).await;
    let cfg = s.dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg,
        format!(
            "state_dir: {state}\napb:\n  workspace_path: {ws}\n",
            state = s.state_dir().display(),
            ws = s.env.apb_workspace.display()
        ),
    )
    .unwrap();
    let map: std::collections::HashMap<String, String> = [
        ("HOME".to_string(), s.dir.path().display().to_string()),
        ("MARKET_URL".to_string(), s.env.market_url.clone()),
        ("APB_BIN".to_string(), s.env.apb_bin.display().to_string()),
        (
            "APB_CONFIG_DIR".to_string(),
            s.env.apb_config_dir.display().to_string(),
        ),
        (
            "NEARBEE_CREDENTIALS".to_string(),
            s.dir
                .path()
                .join("nearbee")
                .join("credentials")
                .display()
                .to_string(),
        ),
    ]
    .into_iter()
    .collect();
    let var: &'static dyn Fn(&str) -> Option<String> =
        Box::leak(Box::new(move |k: &str| map.get(k).cloned()));
    let env = Env {
        cwd: s.dir.path(),
        var,
        timing: Default::default(),
    };
    let cli = Cli {
        config: Some(cfg),
        command: Command::List(ListArgs {
            listing: s.listing(),
        }),
    };
    let mut out = Vec::new();
    let code = nearbee::run(cli, &env, &mut out).await;
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains(&format!("published {HANDLE} ({AGENT_ID}")),
        "{text}"
    );
    assert!(text.contains("0.50"), "{text}");
    // The market was patched and the resolved listing is on disk.
    let bodies = s.server.received_requests().await.unwrap();
    assert!(
        bodies
            .iter()
            .any(|req| req.url.path() == format!("/v1/agents/{AGENT_ID}"))
    );
    assert!(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json"))
            .exists()
    );
}

/// An installed version whose read back lacks `trial_candidates: off`
/// stops the publish before any market call happens.
#[tokio::test]
async fn publishing_stops_before_any_market_call_when_the_read_back_lacks_the_trial_setting() {
    let s = Setup::new().await;
    // The exported playbook read back carries no trial setting.
    std::fs::write(
        s.apb_state().join("readback.json"),
        json!({
            "apb_bundle": 2,
            "id": HANDLE,
            "playbook": "id: repo-audit-fast\nname: x\n"
        })
        .to_string(),
    )
    .unwrap();
    let err = publish(&s.listing(), &s.env, &s.store, &s.state_dir(), s.secrets())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("trial_candidates: off"),
        "the error does not name the setting: {msg}"
    );
    // No market call happened at all.
    assert!(
        s.server.received_requests().await.unwrap().is_empty(),
        "a market call followed a failed read back"
    );
}

/// A market call that fails after apb installed the new version leaves the
/// resolved record on the version apb now runs (codex 4): a previous
/// record naming an older version is replaced before the first market
/// call, so serve never runs an unrecorded version under an old record.
#[tokio::test]
async fn a_market_failure_after_the_install_leaves_the_record_on_the_installed_version() {
    let s = Setup::new().await;
    // A record from an earlier publish, on an older version.
    nearbee_core::listing::write_resolved(
        &s.state_dir(),
        &nearbee_core::listing::ResolvedListing {
            handle: HANDLE.into(),
            agent_id: AGENT_ID.into(),
            playbook_id: HANDLE.into(),
            playbook_version: "0.9.0".into(),
            playbook_sha256: "old".into(),
            price_usd: "1.00".into(),
            sla_seconds: 3600,
            published_at: time::OffsetDateTime::from_unix_timestamp(0).unwrap(),
        },
    )
    .unwrap();
    // The first market call is refused.
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{AGENT_ID}/pricing-plans")))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(fx::refusal("forbidden", "not allowed")),
        )
        .mount(&s.server)
        .await;
    let err = publish(&s.listing(), &s.env, &s.store, &s.state_dir(), s.secrets())
        .await
        .unwrap_err();
    let installed = std::fs::read_to_string(s.apb_state().join("installed-version.txt")).unwrap();
    let msg = err.to_string();
    assert!(msg.contains("forbidden"), "{msg}");
    assert!(
        msg.contains(&format!(
            "playbook version {installed} is installed and recorded"
        )),
        "{msg}"
    );
    assert!(msg.contains("run nearbee list again"), "{msg}");
    let text = std::fs::read_to_string(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json")),
    )
    .unwrap();
    let rec: nearbee_core::listing::ResolvedListing = serde_json::from_str(&text).unwrap();
    assert_eq!(rec.playbook_version, installed);
    assert_ne!(rec.playbook_sha256, "old");
    // Nothing on the market changed.
    assert!(
        s.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET"),
        "a market write followed the refused read"
    );
}
