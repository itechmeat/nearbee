//! CLI tests of `nearbee list` against a wiremock market and a fake apb
//! CLI: the no-market-writes rule and the web-app settings report.
//! Publishing never touches `listing_status` and never writes.

use nearbee::publish::{PublishEnv, publish};
use nearbee::{Cli, Command, Env, ListArgs};
use nearbee_apb::Timing;
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
    [ "$3" = "--out" ] || { echo "error: unexpected argument '$3' found" >&2; exit 2; }
    printf '{"apb_bundle":2,"id":"%s","version":"%s","playbook":"%s"}\n' "$2" "$version" "$playbook" > "$4"
    exit 0 ;;
  *) echo "fake: $*"; exit 2 ;;
esac
"#;

struct Setup {
    dir: tempfile::TempDir,
    /// The market: only the plans read is ever sent here.
    server: MockServer,
    /// Nearbee's own apb instance: the inspection reads the facts here.
    apb: MockServer,
    env: PublishEnv,
    store: Store,
}

impl Setup {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let apb = MockServer::start().await;
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
        // A fake `systemctl` reporting the instance environment: Nearbee's
        // own XDG_CONFIG_HOME, so the test reads no real unit.
        let systemctl = dir.path().join("fake-systemctl.sh");
        std::fs::write(
            &systemctl,
            format!(
                "#!/bin/sh\nprintf '%s\\n' 'Environment=XDG_CONFIG_HOME={}'\n",
                state.join("xdg-config").display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&systemctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let bin = dir.path().join("fake-apb.sh");
        std::fs::write(&bin, FAKE).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = PublishEnv {
            market_url: server.uri(),
            apb_url: apb.uri(),
            apb_api_key: None,
            timing: fast_timing(),
            apb_bin: bin,
            apb_config_dir: state.clone(),
            apb_xdg_config_home: state.join("xdg-config"),
            systemctl_bin: systemctl.clone(),
            apb_workspace: dir.path().join("ws"),
        };
        std::fs::create_dir_all(env.apb_workspace.join(".apb")).unwrap();
        // The instance config the tier check reads: decisions off.
        std::fs::write(
            env.apb_workspace.join(".apb/config.yaml"),
            "decisions:\n  enabled: false\n",
        )
        .unwrap();
        mount_default_apb(&apb, &env.apb_workspace).await;
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
                "handle: {HANDLE}\nname: \"Repo audit (fast)\"\nsource_playbook: {src}\nprice_usd: \"0.50\"\nsla_seconds: 86400\ncategory: dev_tools\ntags: [\"nearbee\", \"example\"]\ndescription: \"Audits a repository.\"\nbio_markdown: \"Nearbee example.\"\ntier: plain\n"
            ),
        )
        .unwrap();
        Self {
            dir,
            server,
            apb,
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

/// The local retention days the tests publish with.
const RETENTION: u32 = 7;
/// The workspace id the apb mock assigns to the test workspace.
const APB_WS: &str = "ws-publish";

/// Short timeouts and no retry delay, so a failing read does not stall a
/// test.
fn fast_timing() -> Timing {
    Timing {
        connect: std::time::Duration::from_secs(1),
        read: std::time::Duration::from_secs(2),
        estimate: std::time::Duration::from_secs(2),
        start: std::time::Duration::from_millis(500),
        abort: std::time::Duration::from_secs(1),
        decide: std::time::Duration::from_secs(1),
        read_retry_delays: vec![std::time::Duration::ZERO; 3],
    }
}

/// A profile detail answer with the given executor YAML.
fn profile_body(name: &str, scope: &str, yaml: &str) -> Value {
    json!({
        "bundle_digest": "sha256:0000",
        "name": name,
        "profile_digest": "sha256:0000",
        "profile_yaml": yaml,
        "scope": scope,
        "soul_md": "Do exactly the task in the prompt and nothing else.\n"
    })
}

/// The profile the shipped source names: `claude` on the owner's
/// subscription, minimal, allowlist.
const NEARBEE_OWN_YAML: &str = "name: nearbee-own\ndescription: the owner's subscription\nexecutor:\n  agent: claude\n  model: claude-haiku-5-5\nenvironment: minimal\nprocess_env:\n  policy: allowlist\n";

async fn mock_profile(server: &MockServer, name: &str, scope: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/profiles/{name}")))
        .and(wiremock::matchers::query_param("scope", scope))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// The apb answers every publish needs: the workspace resolution, the
/// health version, the source's profile (`auto` falls back to the global
/// scope) and the installed playbook read back as the source text.
async fn mount_default_apb(server: &MockServer, ws: &Path) {
    let real_ws = std::fs::canonicalize(ws).unwrap();
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "name": "apb-workspace",
            "parent_workspace_id": null,
            "path": real_ws.display().to_string(),
            "playbook_count": 1,
            "workspace_id": APB_WS,
        }])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version": "1.0.0-beta.6"})))
        .mount(server)
        .await;
    // `auto` reads the project scope and falls back to the global one.
    Mock::given(method("GET"))
        .and(path("/api/profiles/nearbee-own"))
        .and(wiremock::matchers::query_param("scope", "project"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "not found"})))
        .mount(server)
        .await;
    mock_profile(
        server,
        "nearbee-own",
        "global",
        profile_body("nearbee-own", "global", NEARBEE_OWN_YAML),
    )
    .await;
    // The installed playbook carries the listing's handle as its id (the
    // materialised rewrite); the shipped source's id is `repo-audit`.
    let source = std::fs::read_to_string(playbook_path())
        .unwrap()
        .replace("\nid: repo-audit\n", &format!("\nid: {HANDLE}\n"));
    Mock::given(method("GET"))
        .and(path(format!("/api/playbooks/{HANDLE}")))
        .and(wiremock::matchers::query_param("workspace", APB_WS))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": HANDLE, "yaml": source})),
        )
        .mount(server)
        .await;
}

fn playbook_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../playbooks/repo-audit/playbook.yaml")
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

/// The one market call `nearbee list` still makes: the plans read. No
/// PATCH, no plan POST, no make-default: a write would 403 with the agent
/// token anyway, and the mock would hang the request log with it.
async fn mount_market(server: &MockServer, plans: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{AGENT_ID}/pricing-plans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(plans))
        .mount(server)
        .await;
}

/// Every request the server saw was a GET: the market received only reads.
async fn only_reads(server: &MockServer) {
    let bodies = server.received_requests().await.unwrap();
    assert!(
        bodies.iter().all(|req| req.method.as_str() == "GET"),
        "a market write was sent: {:?}",
        bodies.iter().map(|r| r.method.clone()).collect::<Vec<_>>()
    );
}

/// `nearbee list` writes nothing: the market receives only the plans
/// read, the record holds the new keys, and a price mismatch is reported,
/// not failed.
#[tokio::test]
async fn publish_makes_no_market_write_and_reports_the_settings() {
    let s = Setup::new().await;
    let plans = old_default_plan("1.00", 3600);
    mount_market(&s.server, plans).await;
    let (r, rows) = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap();
    assert_eq!(r.handle, HANDLE);
    assert_eq!(r.agent_id, AGENT_ID);
    // The installed version read back equals the derived version of the
    // materialised playbook, numeric `1.0.<n>`.
    assert!(
        r.playbook_version.starts_with("1.0.")
            && r.playbook_version["1.0.".len()..].parse::<u32>().is_ok(),
        "{}",
        r.playbook_version
    );
    assert_eq!(r.price_usd, "0.50");
    // The resolved listing is on disk and carries the new keys.
    let text = std::fs::read_to_string(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json")),
    )
    .unwrap();
    let rec: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rec["protection"], "on", "protection is recorded");
    // The market received only reads, and only the plans call.
    let bodies = s.server.received_requests().await.unwrap();
    assert_eq!(bodies.len(), 1, "{:?}", bodies.len());
    assert_eq!(
        bodies[0].url.path(),
        format!("/v1/agents/{AGENT_ID}/pricing-plans")
    );
    only_reads(&s.server).await;
    // The report: the owner sets the texts in the web app; the price and
    // the SLA are compared with the default plan.
    let fields: Vec<&str> = rows.iter().map(|r| r.field).collect();
    assert_eq!(
        fields,
        [
            "name",
            "description",
            "bio",
            "category",
            "tags",
            "SLA",
            "price"
        ]
    );
    for r in rows.iter().take(5) {
        assert_eq!(
            r.market_value, "not readable with the agent token",
            "{}",
            r.field
        );
        assert_eq!(r.status, "set in web app", "{}", r.field);
    }
    let sla = row(&rows, "SLA");
    assert_eq!(sla.market_value, "3600", "{:?}", sla.market_value);
    assert_eq!(sla.status, "mismatch");
    let price = row(&rows, "price");
    assert_eq!(price.market_value, "1.00");
    assert_eq!(price.status, "mismatch");
}

/// The report row of one field.
fn row<'a>(
    rows: &'a [nearbee::publish::SettingRow],
    field: &str,
) -> &'a nearbee::publish::SettingRow {
    rows.iter()
        .find(|r| r.field == field)
        .unwrap_or_else(|| panic!("no {field} row in {rows:?}"))
}

/// A default plan whose model, price and SLA match reads as ok on both
/// rows, and still nothing is written.
#[tokio::test]
async fn a_matching_plan_reads_as_ok() {
    let s = Setup::new().await;
    let plans = old_default_plan("0.50", 86400);
    mount_market(&s.server, plans).await;
    let (_, rows) = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap();
    assert_eq!(row(&rows, "SLA").status, "ok", "{rows:?}");
    assert_eq!(row(&rows, "price").status, "ok", "{rows:?}");
    only_reads(&s.server).await;
}

/// No plan at all: publishing succeeds, says the price is not set in the
/// web app yet, and a null plan SLA reads as unverified.
#[tokio::test]
async fn no_plan_reads_as_not_set_and_a_null_sla_as_unverified() {
    let s = Setup::new().await;
    mount_market(&s.server, json!([])).await;
    let (_, rows) = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap();
    assert_eq!(row(&rows, "SLA").status, "set in web app", "{rows:?}");
    assert_eq!(row(&rows, "SLA").market_value, "not set in the web app yet");
    assert_eq!(
        row(&rows, "price").market_value,
        "not set in the web app yet"
    );
    // A null plan SLA inherits the agent default, which the token cannot
    // read: unverified, not a mismatch.
    let s2 = Setup::new().await;
    let plan = fx::pricing_plan("plan_old", AGENT_ID, "0.50", None, true);
    mount_market(&s2.server, json!([plan])).await;
    let (_, rows) = publish(
        &s2.listing(),
        &s2.env,
        &s2.store,
        &s2.state_dir(),
        s2.secrets(),
        RETENTION,
    )
    .await
    .unwrap();
    assert_eq!(row(&rows, "SLA").status, "unverified", "{rows:?}");
    assert_eq!(row(&rows, "price").status, "ok", "{rows:?}");
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
            "handle: {HANDLE}\nname: x\nsource_playbook: bad.yaml\nprice_usd: \"0.50\"\nsla_seconds: 86400\ncategory: dev_tools\ntags: [\"t\"]\ndescription: \"d\"\nbio_markdown: \"b\"\ntier: plain\n"
        ),
    )
    .unwrap();
    let err = publish(
        &s.dir.path().join("listing-bad.yaml"),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
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
}

/// The CLI command `nearbee list` drives the same publish through
/// the config file and environment: it prints the summary and exits 0.
/// The apb config directory is the config's `apb.config_dir` (no
/// `APB_CONFIG_DIR` is set), the one serve uses too.
#[tokio::test]
async fn the_cli_publish_list_command_prints_the_summary() {
    let s = Setup::new().await;
    let plans = old_default_plan("1.00", 3600);
    mount_market(&s.server, plans).await;
    let cfg = s.dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg,
        format!(
            "state_dir: {state}\napb:\n  workspace_path: {ws}\n  config_dir: {cd}\n",
            state = s.state_dir().display(),
            ws = s.env.apb_workspace.display(),
            cd = s.env.apb_config_dir.display()
        ),
    )
    .unwrap();
    let map: std::collections::HashMap<String, String> = [
        ("HOME".to_string(), s.dir.path().display().to_string()),
        ("MARKET_URL".to_string(), s.env.market_url.clone()),
        ("APB_BIN".to_string(), s.env.apb_bin.display().to_string()),
        ("APB_URL".to_string(), s.env.apb_url.clone()),
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
        text.contains(&format!("installed {HANDLE} ({AGENT_ID}")),
        "{text}"
    );
    assert!(text.contains("0.50"), "{text}");
    assert!(text.contains("set these in the market web app"), "{text}");
    assert!(text.contains("price"), "{text}");
    assert!(text.contains("mismatch"), "{text}");
    // The resolved listing is on disk and the market only read.
    assert!(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json"))
            .exists()
    );
    only_reads(&s.server).await;
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
    let err = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
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
            scope: None,
            protection: None,
            limits: None,
            tier: Some(nearbee_core::listing::Tier::Plain),
            intake_check: None,
            statement: None,
            statement_sha256: None,
            statement_format: None,
            fingerprint: None,
            evidence_models: Vec::new(),
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
    let err = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
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

/// A listing file with `tier` replaced by the given value and an optional
/// intake check added.
fn listing_with_tier(s: &Setup, tier: &str, intake: Option<&str>) -> PathBuf {
    let intake_line = intake
        .map(|i| format!("intake_check: {i}\n"))
        .unwrap_or_default();
    let file = s.dir.path().join(format!("listing-{tier}.yaml"));
    std::fs::write(
        &file,
        format!(
            "handle: {HANDLE}\nname: x\nsource_playbook: repo-audit-fast.yaml\nprice_usd: \"0.50\"\nsla_seconds: 86400\ncategory: dev_tools\ntags: [\"t\"]\ndescription: \"d\"\nbio_markdown: \"b\"\ntier: {tier}\n{intake_line}"
        ),
    )
    .unwrap();
    file
}

/// An attested listing whose profile falls back to an incognito model
/// fails on the source check, before apb installs anything, and the error
/// names the model (spec `listing-publish`, Tier check at publish,
/// "Source breaks the tier").
#[tokio::test]
async fn an_attested_listing_with_a_bad_fallback_fails_before_install_and_names_the_model() {
    let s = Setup::new().await;
    let yaml = "name: nearbee-own\nexecutor:\n  agent: opencode\n  model: nearai/Qwen/Qwen3.8-27B\nfallbacks:\n- agent: opencode\n  model: claude-haiku-5-5\nenvironment: minimal\nprocess_env:\n  policy: allowlist\n  allow: [NEARAI_API_KEY]\n";
    // The override wins over the setup's default profile: priority 1
    // beats the default 5.
    mount_profile_override(
        &s.apb,
        "nearbee-own",
        "global",
        profile_body("nearbee-own", "global", yaml),
    )
    .await;
    let err = publish(
        &listing_with_tier(&s, "attested", None),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("before installing"), "{msg}");
    assert!(
        msg.contains("claude-haiku-5-5"),
        "the model is not named: {msg}"
    );
    assert!(
        !s.apb_state().join("installed-version.txt").exists(),
        "apb installed something before the check"
    );
    assert!(
        s.server.received_requests().await.unwrap().is_empty(),
        "a market call followed a refused source"
    );
}

/// A listing that passes on the source and fails on the installed read
/// back leaves the previous record and exits non-zero (spec
/// `listing-publish`, "Refused after install").
#[tokio::test]
async fn a_check_failing_only_after_install_leaves_the_old_record() {
    let s = Setup::new().await;
    write_old_record(&s).await;
    // The installed read back holds a node kind the check does not know;
    // the source does not, so only the installed check fails.
    Mock::given(method("GET"))
        .and(path(format!("/api/playbooks/{HANDLE}")))
        .and(wiremock::matchers::query_param("workspace", APB_WS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": HANDLE,
            "yaml": format!("schema: 2\nid: {HANDLE}\nname: x\nversion: 1.0.0\nnodes:\n- id: gate\n  type: human_review\n")
        })))
        .with_priority(1)
        .mount(&s.apb)
        .await;
    let err = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("after installing"), "{msg}");
    assert!(msg.contains("human_review"), "{msg}");
    // The old record is untouched and no market call happened.
    let text = std::fs::read_to_string(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json")),
    )
    .unwrap();
    let rec: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rec["playbook_version"], "0.9.0");
    assert!(rec["fingerprint"].is_null());
    assert!(
        s.server.received_requests().await.unwrap().is_empty(),
        "a market call followed a refused install"
    );
}

/// The installed facts pass the check but differ from the source's: the
/// fingerprints do not match and nothing is recorded.
#[tokio::test]
async fn differing_source_and_installed_fingerprints_fail() {
    let s = Setup::new().await;
    // The installed read back binds another (equally valid) profile.
    Mock::given(method("GET"))
        .and(path(format!("/api/playbooks/{HANDLE}")))
        .and(wiremock::matchers::query_param("workspace", APB_WS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": HANDLE,
            "yaml": format!("schema: 2\nid: {HANDLE}\nname: x\nversion: 1.0.0\ndefaults:\n  profile: nearbee-near\nnodes:\n- id: start\n  type: start\n")
        })))
        .with_priority(1)
        .mount(&s.apb)
        .await;
    mount_profile_override(
        &s.apb,
        "nearbee-near",
        "global",
        profile_body(
            "nearbee-near",
            "global",
            "name: nearbee-near\nexecutor:\n  agent: claude\n  model: claude-haiku-5-5\nenvironment: minimal\nprocess_env:\n  policy: allowlist\n",
        ),
    )
    .await;
    let err = publish(
        &s.listing(),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("installed facts differ from the source"),
        "{msg}"
    );
    assert!(
        !s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json"))
            .exists(),
        "a record was written for facts that differ"
    );
}

/// A passing publish records the tier, the intake check, the statement,
/// its hash and format, the fingerprint and the evidence models, and the
/// market still receives only the plans read (spec `listing-publish`,
/// Materialisation and Tier check at publish).
#[tokio::test]
async fn a_passing_publish_records_the_statement_fields() {
    let s = Setup::new().await;
    let plans = old_default_plan("0.50", 86400);
    mount_market(&s.server, plans).await;
    let (r, _rows) = publish(
        &listing_with_tier(&s, "plain", Some("github_repo")),
        &s.env,
        &s.store,
        &s.state_dir(),
        s.secrets(),
        RETENTION,
    )
    .await
    .unwrap();
    assert_eq!(r.tier, Some(nearbee_core::listing::Tier::Plain));
    assert_eq!(
        r.intake_check,
        Some(nearbee_core::listing::IntakeCheck::GithubRepo)
    );
    let stmt = r.statement.as_ref().unwrap();
    assert!(stmt.starts_with("## Data handling"), "{stmt}");
    assert_eq!(
        r.statement_sha256.as_deref(),
        Some(nearbee_core::journal::sha256_hex(stmt.as_bytes()).as_str())
    );
    assert_eq!(
        r.statement_format,
        Some(nearbee_core::statement::STATEMENT_FORMAT)
    );
    let fp = r.fingerprint.as_ref().unwrap();
    assert!(fp.starts_with("sha256:"), "{fp}");
    assert_eq!(r.evidence_models, vec!["claude-haiku-5-5".to_string()]);
    only_reads(&s.server).await;
}

/// A profile answer that must win over the setup's default mocks: priority
/// 1 beats the default 5 (wiremock: lower priority first, insertion order
/// on ties).
async fn mount_profile_override(server: &MockServer, name: &str, scope: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/profiles/{name}")))
        .and(wiremock::matchers::query_param("scope", scope))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .with_priority(1)
        .mount(server)
        .await;
}

/// The old record of the after-install test.
async fn write_old_record(s: &Setup) {
    std::fs::create_dir_all(s.state_dir().join("listings")).unwrap();
    let rec = json!({
        "handle": HANDLE,
        "agent_id": AGENT_ID,
        "playbook_id": HANDLE,
        "playbook_version": "0.9.0",
        "playbook_sha256": "old",
        "price_usd": "1.00",
        "sla_seconds": 3600,
        "published_at": "1970-01-01T00:00:00Z"
    });
    std::fs::write(
        s.state_dir()
            .join("listings")
            .join(format!("{HANDLE}.json")),
        serde_json::to_string_pretty(&rec).unwrap(),
    )
    .unwrap();
}

/// The settings report prints the tier and the statement block the owner
/// pastes into the bio (spec `listing-publish`, Statement in the settings
/// report).
#[tokio::test]
async fn the_report_prints_the_tier_and_the_statement_block() {
    let s = Setup::new().await;
    let plans = old_default_plan("0.50", 86400);
    mount_market(&s.server, plans).await;
    let cfg = s.dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg,
        format!(
            "state_dir: {state}\napb:\n  url: {url}\n  workspace_path: {ws}\n  config_dir: {cd}\n",
            state = s.state_dir().display(),
            url = s.env.apb_url,
            ws = s.env.apb_workspace.display(),
            cd = s.env.apb_config_dir.display()
        ),
    )
    .unwrap();
    let map: std::collections::HashMap<String, String> = [
        ("HOME".to_string(), s.dir.path().display().to_string()),
        ("MARKET_URL".to_string(), s.env.market_url.clone()),
        ("APB_BIN".to_string(), s.env.apb_bin.display().to_string()),
        ("APB_URL".to_string(), s.env.apb_url.clone()),
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
        timing: fast_timing(),
    };
    let cli = Cli {
        config: Some(cfg),
        command: Command::List(ListArgs {
            listing: listing_with_tier(&s, "plain", None),
        }),
    };
    let mut out = Vec::new();
    let code = nearbee::run(cli, &env, &mut out).await;
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("tier: plain"), "{text}");
    assert!(text.contains("## Data handling"), "{text}");
    only_reads(&s.server).await;
}
