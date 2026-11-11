//! One full hire through `nearbee serve` against a wiremock market and a
//! wiremock apb, using the S1 recorded apb answers.

use std::collections::HashMap;
use std::path::Path;

use nearbee::Env;
use nearbee_core::config::{self, LoadOptions};
use nearbee_core::credentials::{AgentCredentials, Store};
use nearbee_core::journal;
use nearbee_core::secret::Secret;
use nearbee_core::serve::ServeOptions;
use nearbee_market_fixture as fx;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "aat_0123456789abcdef0123456789abcdef";
const HANDLE: &str = "repo-audit-fast";
const WS: &str = "ws-c0188762df734bf4a1f03f8db22f19d7";

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nearbee-apb/tests/fixtures/apb")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn env_of(vars: &[(&str, String)]) -> (Env<'static>, Vec<u8>) {
    let map: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .chain([("HOME".to_string(), "/home/u".to_string())])
        .collect();
    let var: &'static dyn Fn(&str) -> Option<String> =
        Box::leak(Box::new(move |k: &str| map.get(k).cloned()));
    (
        Env {
            cwd: Path::new("/"),
            var,
            timing: Default::default(),
        },
        Vec::new(),
    )
}

/// A deliverable worktree with a valid result manifest.
fn make_worktree(ws: &Path, row_id: &str) -> String {
    let wt = ws.join("work").join(journal::safe_id(row_id));
    std::fs::create_dir_all(wt.join("out")).unwrap();
    let md = "# Audit\n\nThe repository looks healthy.\n";
    std::fs::write(wt.join("out").join("deliverable.md"), md).unwrap();
    let manifest = json!({
        "version": 1,
        "markdown": {"path": "deliverable.md", "size": md.len(),
                     "sha256": journal::sha256_hex(md.as_bytes())}
    });
    std::fs::write(
        wt.join("out").join("result.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    md.to_owned()
}

#[tokio::test]
async fn one_full_hire_is_taken_run_and_submitted() {
    let dir = tempfile::tempdir().unwrap();
    let market = MockServer::start().await;
    let apb = MockServer::start().await;
    let ws = dir.path().join("apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();

    // The credentials store and the resolved listing.
    let store = Store::at(dir.path().join("nearbee").join("credentials"));
    store
        .put(
            HANDLE,
            AgentCredentials {
                agent_id: "ag_123".into(),
                token: Secret::new(TOKEN),
            },
        )
        .unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(state_dir.join("listings")).unwrap();

    // The config: apb at the wiremock server, the workspace in the tempdir.
    let cfg_path = dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "state_dir: {}\napb:\n  url: {}\n  workspace_path: {}\n",
            state_dir.display(),
            apb.uri(),
            ws.display()
        ),
    )
    .unwrap();

    // The apb side: recorded answers with the real workspace id.
    let projects = fixture("projects")["response"]["body"].to_string();
    let projects: Value = serde_json::from_str(&projects.replace(
        "<home>/.local/share/nearbee/apb-workspace",
        &ws.to_string_lossy(),
    ))
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects))
        .mount(&apb)
        .await;
    // The drift pass inspects the listing each round: a serveable S4
    // record built from the same facts does not drift.
    let sys = mount_inspection(&apb, &ws, dir.path()).await;
    let (record, statement) = s4_record(&apb, &ws, &sys, "1.0.0", b"playbook").await;
    std::fs::write(
        state_dir.join("listings").join(format!("{HANDLE}.json")),
        record,
    )
    .unwrap();
    let run_id = "repo-audit-fast-1";
    // The run search by execution id answers complete and empty: no run
    // with this execution id exists, so the machine starts it.
    Mock::given(method("GET"))
        .and(path("/api/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "runs": [],
            "not_indexed": 0,
            "withheld": 0
        })))
        .mount(&apb)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/playbooks/{HANDLE}/run")))
        .and(query_param("workspace", WS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": run_id})))
        .expect(1)
        .mount(&apb)
        .await;
    // apb's playbook list names the published version as current, so the
    // start passes the version check. The list is read once per start
    // gate (a read; the count is not pinned).
    Mock::given(method("GET"))
        .and(path("/api/playbooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": HANDLE,
            "name": "Repo audit (fast)",
            "current": "1.0.0",
            "versions": ["1.0.0"],
            "workspace_id": WS,
        }])))
        .mount(&apb)
        .await;
    // First poll running, then succeeded; the run reports the published
    // version.
    let running = json!({
        "run_id": run_id,
        "run_status": "running",
        "playbook": HANDLE,
        "version": "1.0.0",
    });
    Mock::given(method("GET"))
        .and(path(format!("/api/runs/{run_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(running))
        .up_to_n_times(1)
        .mount(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/runs/{run_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "run_status": "succeeded",
            "playbook": HANDLE,
            "version": "1.0.0",
        })))
        .mount(&apb)
        .await;

    // The market side.
    let row = fx::assignment_item(
        "hire-1",
        "in_progress",
        json!({ "slaDeadlineAt": "2027-11-20T12:00:00Z" }),
        fx::instance(
            "WorkerJobBrief",
            // No attachments: the schema's example download url is not a
            // market path, which serve rightly refuses to fetch.
            json!({ "description": "Audit the repository.", "attachments": [] }),
        ),
    );
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![row])))
        .mount(&market)
        .await;
    // Exactly one market start and one submit across all ten rounds (O1):
    // the machine never repeats a landed write.
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-1/start"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fx::write_answer("StartAssignmentResponse")),
        )
        .expect(1)
        .mount(&market)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-1/messages"))
        .respond_with(ResponseTemplate::new(201).set_body_json(fx::write_answer("ThreadMessage")))
        .mount(&market)
        .await;
    // The deliverable ends with the statement snapshotted at the start.
    let markdown = format!("{}\n\n---\n\n{statement}", make_worktree(&ws, "hire-1"));
    let submitted = Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-1/submit"))
        .and(body_partial_json(json!({"deliverableMarkdown": markdown})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::write_answer("JobAssignment")))
        .expect(1)
        .mount_as_scoped(&market)
        .await;

    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 0,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
        statement_retention_days: 7,
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    // MARKET_URL must reach the market wiremock; env_of cannot see the
    // server handle after the move, so build vars first.
    let vars = vec![
        ("MARKET_URL", market.uri()),
        ("SYSTEMCTL_BIN", sys.display().to_string()),
    ];
    let (env, _ignored) = env_of(&vars);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(10)).await;
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&buf));
    // The journal shows the whole hire.
    let text = std::fs::read_to_string(journal::journal_path(&state_dir, "hire-1")).unwrap();
    assert!(text.contains("market_start"), "{text}");
    assert!(text.contains("run_start"), "{text}");
    assert!(text.contains("market_submit"), "{text}");
    assert!(text.contains("delivered"), "{text}");
    drop(submitted);
}

/// A revoked token stops only that agent's polling: over two rounds the
/// revoked agent's assignments endpoint is read exactly once and the other
/// agent keeps being served.
#[tokio::test]
async fn a_revoked_token_is_polled_once_and_the_other_agent_keeps_serving() {
    const REVOKED: &str = "aat_fedcba9876543210fedcba9876543210";
    const REVOKED_HANDLE: &str = "other-agent";
    let dir = tempfile::tempdir().unwrap();
    let market = MockServer::start().await;
    let apb = MockServer::start().await;
    let ws = dir.path().join("apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();

    let store = Store::at(dir.path().join("nearbee").join("credentials"));
    for (handle, token) in [(HANDLE, TOKEN), (REVOKED_HANDLE, REVOKED)] {
        store
            .put(
                handle,
                AgentCredentials {
                    agent_id: "ag_123".into(),
                    token: Secret::new(token),
                },
            )
            .unwrap();
        let state_dir = dir.path().join("state");
        std::fs::create_dir_all(state_dir.join("listings")).unwrap();
        std::fs::write(
            state_dir.join("listings").join(format!("{handle}.json")),
            json!({
                "handle": handle,
                "agent_id": "ag_123",
                "playbook_id": handle,
                "playbook_version": "1.0.0",
                "playbook_sha256": journal::sha256_hex(b"playbook"),
                "price_usd": "0.50",
                "sla_seconds": 86_400,
                "published_at": "2026-11-09T12:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
    }

    let cfg_path = dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "state_dir: {}\napb:\n  url: {}\n  workspace_path: {}\n",
            dir.path().join("state").display(),
            apb.uri(),
            ws.display()
        ),
    )
    .unwrap();

    // apb answers its projects list to every connect (the recorded shape).
    let projects = fixture("projects")["response"]["body"].to_string();
    let projects: Value = serde_json::from_str(&projects.replace(
        "<home>/.local/share/nearbee/apb-workspace",
        &ws.to_string_lossy(),
    ))
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects))
        .mount(&apb)
        .await;

    // The revoked agent's token is answered 401 exactly once: the loop
    // must never poll it again.
    let revoked = Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .and(header(
            "Authorization",
            format!("Bearer {REVOKED}").as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(fx::refusal("unauthorized", "bad token")),
        )
        .expect(1)
        .mount_as_scoped(&market)
        .await;
    // The healthy agent is polled on both rounds (no hires left).
    let healthy = Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .and(header("Authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![])))
        .up_to_n_times(10)
        .mount_as_scoped(&market)
        .await;

    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 0,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
        statement_retention_days: 7,
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    let (env, _) = env_of(&[("MARKET_URL", market.uri())]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(10)).await;
    let text = String::from_utf8_lossy(&buf);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("was revoked"),
        "the operator is not told: {text}"
    );
    drop(revoked);
    drop(healthy);
}

/// The XDG config directory `config::load` gives with `HOME=/home/u`.
const TEST_XDG: &str = "/home/u/.local/share/nearbee/xdg-config";

/// Makes the wiremock apb answer the drift pass's inspection of `HANDLE`
/// (a start-and-finish playbook, health at the recorded release),
/// decisions off in the workspace, and a fake `systemctl` that reports
/// Nearbee's own `XDG_CONFIG_HOME`; returns that `systemctl`.
async fn mount_inspection(apb: &MockServer, ws: &Path, dir: &Path) -> std::path::PathBuf {
    Mock::given(method("GET"))
        .and(path(format!("/api/playbooks/{HANDLE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": HANDLE,
            "yaml": format!(
                "schema: 2\nid: {HANDLE}\nversion: 1.0.0\nnodes:\n- id: start\n  type: start\n- id: done\n  type: finish\n"
            ),
        })))
        .mount(apb)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(fixture("health_live")["response"]["body"].clone()),
        )
        .mount(apb)
        .await;
    std::fs::create_dir_all(ws.join(".apb")).unwrap();
    std::fs::write(
        ws.join(".apb/config.yaml"),
        "decisions:\n  enabled: false\n",
    )
    .unwrap();
    let sys = dir.join("fake-systemctl.sh");
    std::fs::write(
        &sys,
        format!("#!/bin/sh\nprintf '%s\\n' 'Environment=XDG_CONFIG_HOME={TEST_XDG}'\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sys, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    sys
}

/// A serveable `plain` record of `HANDLE` at `version` built from the
/// facts the mounted inspection gives (so the drift pass finds no drift),
/// as JSON, with its statement.
async fn s4_record(
    apb: &MockServer,
    ws: &Path,
    sys: &Path,
    version: &str,
    digest_of: &[u8],
) -> (String, String) {
    let runner = nearbee_apb::ApbRunner::connect(
        nearbee_apb::ApbOptions {
            url: apb.uri(),
            api_key: None,
            redact: Vec::new(),
            timing: Default::default(),
            bin: "apb".into(),
            config_dir: "/home/u/.local/share/nearbee/apb-config".into(),
            systemctl_bin: sys.to_path_buf(),
            xdg_config_home: TEST_XDG.into(),
        },
        ws,
    )
    .await
    .unwrap();
    let facts =
        nearbee_core::Runner::inspect(&runner, &nearbee_core::runner::PlaybookId::new(HANDLE))
            .await
            .unwrap();
    let tier = nearbee_core::listing::Tier::Plain;
    let statement = nearbee_core::statement::render(tier, &facts, 7);
    let mut record: Value = serde_json::from_str(&listing_record(version, digest_of)).unwrap();
    record["tier"] = json!("plain");
    record["statement_sha256"] = json!(journal::sha256_hex(statement.as_bytes()));
    record["statement"] = json!(statement);
    record["statement_format"] = json!(nearbee_core::statement::STATEMENT_FORMAT);
    record["fingerprint"] = json!(nearbee_core::statement::fingerprint(tier, &facts));
    (record.to_string(), statement)
}

/// The resolved listing record of `HANDLE` at one playbook version.
fn listing_record(version: &str, digest_of: &[u8]) -> String {
    json!({
        "handle": HANDLE,
        "agent_id": "ag_123",
        "playbook_id": HANDLE,
        "playbook_version": version,
        "playbook_sha256": journal::sha256_hex(digest_of),
        "price_usd": "0.50",
        "sla_seconds": 86_400,
        "published_at": "2026-11-09T12:00:00Z"
    })
    .to_string()
}

/// A listing republished as 2.0.0 while serve runs on 1.0.0 is taken for
/// the next hire without a restart: the first round polls no hire and the
/// republish lands during it; the second round reads the new record, so
/// the hire passes the version check against apb's 2.0.0 and is delivered
/// with 2.0.0 in its run intent (round 6, codex 2).
#[tokio::test]
async fn a_republish_during_serve_is_taken_for_the_next_hire_without_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let market = MockServer::start().await;
    let apb = MockServer::start().await;
    let ws = dir.path().join("apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();

    let store = Store::at(dir.path().join("nearbee").join("credentials"));
    store
        .put(
            HANDLE,
            AgentCredentials {
                agent_id: "ag_123".into(),
                token: Secret::new(TOKEN),
            },
        )
        .unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(state_dir.join("listings")).unwrap();
    let record_path = state_dir.join("listings").join(format!("{HANDLE}.json"));

    let cfg_path = dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "state_dir: {}\napb:\n  url: {}\n  workspace_path: {}\n",
            state_dir.display(),
            apb.uri(),
            ws.display()
        ),
    )
    .unwrap();

    // apb after the republish: 2.0.0 is current and the run reports it.
    let projects = fixture("projects")["response"]["body"].to_string();
    let projects: Value = serde_json::from_str(&projects.replace(
        "<home>/.local/share/nearbee/apb-workspace",
        &ws.to_string_lossy(),
    ))
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects))
        .mount(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "runs": [],
            "not_indexed": 0,
            "withheld": 0
        })))
        .mount(&apb)
        .await;
    // Both records are serveable S4 records over the same facts.
    let sys = mount_inspection(&apb, &ws, dir.path()).await;
    let (v1, _) = s4_record(&apb, &ws, &sys, "1.0.0", b"playbook v1").await;
    let (v2, statement) = s4_record(&apb, &ws, &sys, "2.0.0", b"playbook v2").await;
    std::fs::write(&record_path, v1).unwrap();
    let run_id = "repo-audit-fast-2";
    Mock::given(method("POST"))
        .and(path(format!("/api/playbooks/{HANDLE}/run")))
        .and(query_param("workspace", WS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": run_id})))
        .up_to_n_times(10)
        .mount(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/playbooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": HANDLE,
            "name": "Repo audit (fast)",
            "current": "2.0.0",
            "versions": ["1.0.0", "2.0.0"],
            "workspace_id": WS,
        }])))
        .mount(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/runs/{run_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "run_status": "succeeded",
            "playbook": HANDLE,
            "version": "2.0.0",
        })))
        .mount(&apb)
        .await;

    // Round one: no hire yet, and the operator republishes meanwhile.
    let republish_to = record_path.clone();
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(move |_req: &wiremock::Request| {
            std::fs::write(&republish_to, &v2).unwrap();
            ResponseTemplate::new(200).set_body_json(fx::assignments(vec![]))
        })
        .up_to_n_times(1)
        .mount(&market)
        .await;
    // Round two: the next hire arrives.
    let row = fx::assignment_item(
        "hire-2",
        "in_progress",
        json!({ "slaDeadlineAt": "2027-11-20T12:00:00Z" }),
        fx::instance(
            "WorkerJobBrief",
            // No attachments: the schema's example download url is not a
            // market path, which serve rightly refuses to fetch.
            json!({ "description": "Audit the repository.", "attachments": [] }),
        ),
    );
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![row])))
        .mount(&market)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-2/start"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fx::write_answer("StartAssignmentResponse")),
        )
        .up_to_n_times(10)
        .mount(&market)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-2/messages"))
        .respond_with(ResponseTemplate::new(201).set_body_json(fx::write_answer("ThreadMessage")))
        .mount(&market)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-2/decline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::write_answer("JobAssignment")))
        .expect(0)
        .mount(&market)
        .await;
    let markdown = format!("{}\n\n---\n\n{statement}", make_worktree(&ws, "hire-2"));
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-2/submit"))
        .and(body_partial_json(json!({"deliverableMarkdown": markdown})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::write_answer("JobAssignment")))
        .up_to_n_times(10)
        .mount(&market)
        .await;

    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    let opts = ServeOptions {
        // No wait between the two rounds.
        poll_interval_secs: 0,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
        statement_retention_days: 7,
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    let (env, _) = env_of(&[
        ("MARKET_URL", market.uri()),
        ("SYSTEMCTL_BIN", sys.display().to_string()),
    ]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(10)).await;
    let out = String::from_utf8_lossy(&buf);
    assert_eq!(code, 0, "{out}");
    let text = std::fs::read_to_string(journal::journal_path(&state_dir, "hire-2")).unwrap();
    assert!(text.contains("delivered"), "{out}\n{text}");
    assert!(
        text.contains(r#""playbook_version":"2.0.0""#)
            && text.contains(&journal::sha256_hex(b"playbook v2")),
        "{text}"
    );
    assert!(!text.contains("1.0.0"), "{text}");
}

/// O3: with apb unreachable the round still runs its journal-driven and
/// time-based steps: a waiting hire past its wall clock is declined
/// exactly once, and nothing is started.
#[tokio::test]
async fn with_apb_down_a_hire_past_its_deadline_is_still_declined() {
    let dir = tempfile::tempdir().unwrap();
    let market = MockServer::start().await;
    // An apb address nobody answers on.
    let apb_uri = {
        let gone = MockServer::start().await;
        gone.uri()
    };
    let ws = dir.path().join("apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();
    let store = Store::at(dir.path().join("nearbee").join("credentials"));
    store
        .put(
            HANDLE,
            AgentCredentials {
                agent_id: "ag_123".into(),
                token: Secret::new(TOKEN),
            },
        )
        .unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(state_dir.join("listings")).unwrap();
    std::fs::write(
        state_dir.join("listings").join(format!("{HANDLE}.json")),
        listing_record("1.0.0", b"playbook"),
    )
    .unwrap();
    let cfg_path = dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "state_dir: {}\napb:\n  url: {}\n  workspace_path: {}\n",
            state_dir.display(),
            apb_uri,
            ws.display()
        ),
    )
    .unwrap();
    // An owner-started hire whose SLA deadline is long past.
    let row = fx::assignment_item(
        "hire-3",
        "in_progress",
        json!({ "slaDeadlineAt": "2026-01-01T12:00:00Z",
                "startedAt": "2025-12-31T12:00:00Z" }),
        fx::instance(
            "WorkerJobBrief",
            json!({ "description": "Audit the repository.", "attachments": [] }),
        ),
    );
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![row])))
        .mount(&market)
        .await;
    // The row was started (here by the owner): work counts as started,
    // so the decline is `failed`, never `rejected` (R2-12).
    let declined = Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-3/decline"))
        .and(body_partial_json(json!({"reason": "failed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::write_answer("JobAssignment")))
        .expect(1)
        .mount_as_scoped(&market)
        .await;
    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 0,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
        statement_retention_days: 7,
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    let (env, _) = env_of(&[("MARKET_URL", market.uri())]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(3)).await;
    let out = String::from_utf8_lossy(&buf);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("error: apb"), "{out}");
    drop(declined);
    let text = std::fs::read_to_string(journal::journal_path(&state_dir, "hire-3")).unwrap();
    assert!(!text.contains("run_start"), "{text}");
}

/// R2-2: with every assignments read failing (the only agent's token is
/// revoked), the journal pass still runs: a hire with an active run past
/// its recorded wall clock is observed and stopped, and its `failed`
/// decline waits in the journal for the row.
#[tokio::test]
async fn with_every_read_failing_an_active_run_is_still_stopped_at_its_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let market = MockServer::start().await;
    let apb = MockServer::start().await;
    let ws = dir.path().join("apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();
    let store = Store::at(dir.path().join("nearbee").join("credentials"));
    store
        .put(
            HANDLE,
            AgentCredentials {
                agent_id: "ag_123".into(),
                token: Secret::new(TOKEN),
            },
        )
        .unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(state_dir.join("listings")).unwrap();
    std::fs::write(
        state_dir.join("listings").join(format!("{HANDLE}.json")),
        listing_record("1.0.0", b"playbook"),
    )
    .unwrap();
    let cfg_path = dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "state_dir: {}\napb:\n  url: {}\n  workspace_path: {}\n",
            state_dir.display(),
            apb.uri(),
            ws.display()
        ),
    )
    .unwrap();
    let projects = fixture("projects")["response"]["body"].to_string();
    let projects: Value = serde_json::from_str(&projects.replace(
        "<home>/.local/share/nearbee/apb-workspace",
        &ws.to_string_lossy(),
    ))
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects))
        .mount(&apb)
        .await;
    let run_id = "repo-audit-fast-9";
    let observed = Mock::given(method("GET"))
        .and(path(format!("/api/runs/{run_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "run_status": "running",
            "playbook": HANDLE,
            "version": "1.0.0",
        })))
        .expect(1..)
        .mount_as_scoped(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "runs": [], "not_indexed": 0, "withheld": 0
        })))
        .mount(&apb)
        .await;
    // The only agent's token is revoked: no row is read in any round.
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(fx::refusal("unauthorized", "bad token")),
        )
        .mount(&market)
        .await;
    // A hire with an active run whose recorded deadline is long past.
    {
        let mut w =
            journal::Writer::open(journal::journal_path(&state_dir, "hire-9"), "hire-9").unwrap();
        w.set_generation(1);
        w.append(journal::Event::Received(journal::Received::from_brief(
            "brief",
        )))
        .unwrap();
        let op = uuid::Uuid::now_v7();
        w.append(journal::Event::Intent(journal::Intent {
            operation_id: op,
            kind: "run_start".into(),
            execution: 1,
            request_digest: "x".into(),
            refs: std::collections::BTreeMap::from([
                ("execution_id".to_owned(), "hire-9-1".to_owned()),
                (
                    "sla_deadline_at".to_owned(),
                    "2026-01-01T12:00:00Z".to_owned(),
                ),
            ]),
        }))
        .unwrap();
        w.append(journal::Event::Result(journal::OpResult {
            operation_id: op,
            outcome: "done".into(),
            external_ids: std::collections::BTreeMap::from([(
                "run_id".to_owned(),
                run_id.to_owned(),
            )]),
        }))
        .unwrap();
        w.append(journal::Event::RunObserved(journal::RunObserved {
            attempts: Vec::new(),
            run_id: run_id.into(),
            execution: 1,
            status: "running".into(),
            playbook_version: Some("1.0.0".into()),
            cost_usd: None,
            cost_complete: None,
            budget_usd: None,
        }))
        .unwrap();
    }
    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 0,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
        statement_retention_days: 7,
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    // The abort runs `apb stop`; a binary that does not exist keeps this
    // test away from any real apb (the call fails, the intent stays).
    let no_apb = dir.path().join("no-apb");
    let (env, _) = env_of(&[
        ("MARKET_URL", market.uri()),
        ("APB_BIN", no_apb.to_string_lossy().to_string()),
    ]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(2)).await;
    let out = String::from_utf8_lossy(&buf);
    assert_eq!(code, 0, "{out}");
    drop(observed);
    let j = journal::read(
        &std::fs::read_to_string(journal::journal_path(&state_dir, "hire-9")).unwrap(),
    )
    .unwrap();
    assert!(
        j.open_intents
            .iter()
            .any(|i| i.kind == "run_abort" && i.refs["run_id"] == run_id),
        "the run was not stopped: {out}\n{:?}",
        j.records
    );
    assert!(
        j.open_intents
            .iter()
            .any(|i| i.kind == "market_decline" && i.refs["reason"] == "failed"),
        "{:?}",
        j.open_intents
    );
}

/// R4-7: with no loadable listing and no credentials at all, serve still
/// recovers the journals on disk: an active run past its recorded wall
/// clock is stopped through a controlled `apb` executable, and serve
/// exits only once that local work is settled.
#[tokio::test]
async fn with_no_listing_loaded_the_local_work_still_settles_before_exit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let apb = MockServer::start().await;
    let ws = dir.path().join("apb-workspace");
    std::fs::create_dir_all(&ws).unwrap();
    let store = Store::at(dir.path().join("nearbee").join("credentials"));
    let state_dir = dir.path().join("state");
    let cfg_path = dir.path().join("nearbee.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "state_dir: {}\napb:\n  url: {}\n  workspace_path: {}\n",
            state_dir.display(),
            apb.uri(),
            ws.display()
        ),
    )
    .unwrap();
    let projects = fixture("projects")["response"]["body"].to_string();
    let projects: Value = serde_json::from_str(&projects.replace(
        "<home>/.local/share/nearbee/apb-workspace",
        &ws.to_string_lossy(),
    ))
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects))
        .mount(&apb)
        .await;
    let run_id = "repo-audit-fast-11";
    // The run reads `running` until the fake `apb stop` has been called,
    // then `aborted`.
    Mock::given(method("GET"))
        .and(path(format!("/api/runs/{run_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "run_status": "running",
            "playbook": HANDLE,
            "version": "1.0.0",
        })))
        .up_to_n_times(1)
        .mount(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/runs/{run_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "run_status": "aborted",
            "playbook": HANDLE,
            "version": "1.0.0",
        })))
        .mount(&apb)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "runs": [], "not_indexed": 0, "withheld": 0
        })))
        .mount(&apb)
        .await;
    {
        let mut w =
            journal::Writer::open(journal::journal_path(&state_dir, "hire-11"), "hire-11").unwrap();
        w.set_generation(1);
        w.append(journal::Event::Received(journal::Received::from_brief(
            "brief",
        )))
        .unwrap();
        let op = uuid::Uuid::now_v7();
        w.append(journal::Event::Intent(journal::Intent {
            operation_id: op,
            kind: "run_start".into(),
            execution: 1,
            request_digest: "x".into(),
            refs: std::collections::BTreeMap::from([
                ("execution_id".to_owned(), "hire-11-1".to_owned()),
                (
                    "sla_deadline_at".to_owned(),
                    "2026-01-01T12:00:00Z".to_owned(),
                ),
            ]),
        }))
        .unwrap();
        w.append(journal::Event::Result(journal::OpResult {
            operation_id: op,
            outcome: "done".into(),
            external_ids: std::collections::BTreeMap::from([(
                "run_id".to_owned(),
                run_id.to_owned(),
            )]),
        }))
        .unwrap();
        w.append(journal::Event::RunObserved(journal::RunObserved {
            attempts: Vec::new(),
            run_id: run_id.into(),
            execution: 1,
            status: "running".into(),
            playbook_version: Some("1.0.0".into()),
            cost_usd: None,
            cost_complete: None,
            budget_usd: None,
        }))
        .unwrap();
    }
    // A controlled `apb`: it records its arguments and succeeds.
    let calls = dir.path().join("apb-calls.txt");
    let fake = dir.path().join("fake-apb");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", calls.display()),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |k| (k == "HOME").then(|| "/home/u".to_owned()),
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 0,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
        statement_retention_days: 7,
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    let (env, _) = env_of(&[
        ("MARKET_URL", "http://127.0.0.1:9".to_owned()),
        ("APB_BIN", fake.to_string_lossy().to_string()),
    ]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(6)).await;
    let out = String::from_utf8_lossy(&buf);
    let stops = std::fs::read_to_string(&calls).unwrap_or_default();
    assert!(
        stops.contains(&format!("stop {run_id}")),
        "the run was never stopped: {out}\n{stops}"
    );
    let j = journal::read(
        &std::fs::read_to_string(journal::journal_path(&state_dir, "hire-11")).unwrap(),
    )
    .unwrap();
    assert!(
        !j.open_intents.iter().any(|i| i.kind == "run_abort"),
        "the abort was never confirmed: {:?}",
        j.records
    );
    assert!(
        j.open_intents
            .iter()
            .any(|i| i.kind == "market_decline" && i.refs["reason"] == "failed"),
        "{:?}",
        j.open_intents
    );
    assert!(out.contains("nothing to serve"), "{out}");
    assert_eq!(code, 1, "{out}");
}
