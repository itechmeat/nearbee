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
    let wt = ws.join("work").join(journal::safe_id(row_id)).join("1");
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
    std::fs::write(
        state_dir.join("listings").join(format!("{HANDLE}.json")),
        json!({
            "handle": HANDLE,
            "agent_id": "ag_123",
            "playbook_id": HANDLE,
            "playbook_version": "1.0.0",
            "playbook_sha256": journal::sha256_hex(b"playbook"),
            "price_usd": "0.50",
            "sla_seconds": 86_400,
            "published_at": "2026-11-09T12:00:00Z"
        })
        .to_string(),
    )
    .unwrap();

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
    // start passes the version check.
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
            json!({ "description": "Audit the repository." }),
        ),
    );
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![row])))
        .mount(&market)
        .await;
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
    let markdown = make_worktree(&ws, "hire-1");
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
        |_k| None,
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 45,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    // MARKET_URL must reach the market wiremock; env_of cannot see the
    // server handle after the move, so build vars first.
    let vars = vec![("MARKET_URL", market.uri())];
    let (env, _ignored) = env_of(&vars);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(1)).await;
    eprintln!("SERVE LINES: {}", String::from_utf8_lossy(&buf));
    let jp = journal::journal_path(&state_dir, "hire-1");
    eprintln!("JOURNAL PATH: {} exists={}", jp.display(), jp.exists());
    if let Ok(t) = std::fs::read_to_string(&jp) {
        eprintln!("JOURNAL: {t}");
    }
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&buf));
    // The journal shows the whole hire.
    let text = std::fs::read_to_string(journal::journal_path(&state_dir, "hire-1")).unwrap();
    eprintln!("JOURNAL: {text}");
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
        .expect(2)
        .mount_as_scoped(&market)
        .await;

    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |_k| None,
    )
    .unwrap();
    let opts = ServeOptions {
        poll_interval_secs: 45,
        run_poll_interval: std::time::Duration::ZERO,
        progress_every: None,
        reject_handle: None,
        sla_margin_minutes: 10,
        forbidden: Vec::new(),
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    let (env, _) = env_of(&[("MARKET_URL", market.uri())]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(2)).await;
    let text = String::from_utf8_lossy(&buf);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("was revoked"),
        "the operator is not told: {text}"
    );
    drop(revoked);
    drop(healthy);
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
    std::fs::write(&record_path, listing_record("1.0.0", b"playbook v1")).unwrap();

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
    let run_id = "repo-audit-fast-2";
    Mock::given(method("POST"))
        .and(path(format!("/api/playbooks/{HANDLE}/run")))
        .and(query_param("workspace", WS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"run_id": run_id})))
        .expect(1)
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
            std::fs::write(&republish_to, listing_record("2.0.0", b"playbook v2")).unwrap();
            ResponseTemplate::new(200).set_body_json(fx::assignments(vec![]))
        })
        .up_to_n_times(1)
        .expect(1)
        .mount(&market)
        .await;
    // Round two: the next hire arrives.
    let row = fx::assignment_item(
        "hire-2",
        "in_progress",
        json!({ "slaDeadlineAt": "2027-11-20T12:00:00Z" }),
        fx::instance(
            "WorkerJobBrief",
            json!({ "description": "Audit the repository." }),
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
        .expect(1)
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
    let markdown = make_worktree(&ws, "hire-2");
    Mock::given(method("POST"))
        .and(path("/v1/assignments/hire-2/submit"))
        .and(body_partial_json(json!({"deliverableMarkdown": markdown})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::write_answer("JobAssignment")))
        .expect(1)
        .mount(&market)
        .await;

    let cfg = config::load(
        LoadOptions {
            file: Some(&cfg_path),
            cwd: dir.path(),
        },
        |_k| None,
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
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut lines = nearbee::Lines::new(&mut buf);
    let (env, _) = env_of(&[("MARKET_URL", market.uri())]);
    let code = nearbee::serve_loop(&cfg, &store, &opts, &env, &mut lines, Some(2)).await;
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
