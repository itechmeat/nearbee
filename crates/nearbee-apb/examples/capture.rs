//! Captures apb REST fixtures from Nearbee's own live apb instance.
//!
//! ```text
//! cargo run -p nearbee-apb --example capture -- <name>
//! ```
//!
//! Names: `projects`, `playbooks`, `estimate`, `start` (also captures
//! `run_running` right after the start), `run_succeeded` (waits for the run
//! of `start` to end), `find_hire`, `find_execution`, `refusal_launch`,
//! `refusal_budget`, `refusal_invalid`, `run_404`, `all`.
//!
//! Environment: `APB_URL` (default `http://127.0.0.1:7322`),
//! `NEARBEE_WORKSPACE` (default `~/.local/share/nearbee/apb-workspace`),
//! `NEARBEE_CAPTURE_STATE` (state between captures, default
//! `~/.cache/nearbee-apb-capture.json`), `NEARBEE_CAPTURE_PLAYBOOK`
//! (default `smoke-own`, the owner's own subscription: no Cloud spend).
//!
//! Fixtures go to `crates/nearbee-apb/tests/fixtures/apb/<name>.json`. Bodies
//! are scrubbed: the home directory becomes `<home>`, an `Authorization`
//! header is never stored. Tests never write fixtures.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const FIXTURE_PLAYBOOK_DEFAULT: &str = "smoke-own";

struct Ctx {
    http: reqwest::Client,
    url: String,
    home: String,
    workspace_path: PathBuf,
    state_path: PathBuf,
    out_dir: PathBuf,
    playbook: String,
    apb_version: String,
}

#[tokio::main]
async fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: capture <name>|all");
        std::process::exit(2);
    });
    let home = std::env::var("HOME").expect("HOME");
    let url = std::env::var("APB_URL").unwrap_or_else(|_| "http://127.0.0.1:7322".into());
    assert!(
        !url.contains(":7321"),
        "7321 is the operator's apb; capture only from Nearbee's own instance"
    );
    let workspace_path = std::env::var("NEARBEE_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(&home).join(".local/share/nearbee/apb-workspace"));
    let state_path = std::env::var("NEARBEE_CAPTURE_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(&home).join(".cache/nearbee-apb-capture.json"));
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/apb");
    std::fs::create_dir_all(&out_dir).unwrap();
    let apb_version = std::process::Command::new("apb")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    let ctx = Ctx {
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
        url,
        home,
        workspace_path,
        state_path,
        out_dir,
        playbook: std::env::var("NEARBEE_CAPTURE_PLAYBOOK")
            .unwrap_or_else(|_| FIXTURE_PLAYBOOK_DEFAULT.into()),
        apb_version,
    };
    let names: Vec<&str> = if name == "all" {
        vec![
            "projects",
            "playbooks",
            "estimate",
            "refusal_launch",
            "refusal_budget",
            "refusal_invalid",
            "run_404",
            "start",
            "run_succeeded",
            "find_hire",
            "find_execution",
        ]
    } else {
        vec![name.as_str()]
    };
    for n in names {
        capture(&ctx, n).await;
    }
}

async fn workspace_id(ctx: &Ctx) -> String {
    let projects: Value = ctx
        .http
        .get(format!("{}/api/projects", ctx.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let want = ctx.workspace_path.canonicalize().unwrap();
    projects
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p["path"]
                .as_str()
                .is_some_and(|s| Path::new(s).canonicalize().ok().as_deref() == Some(&want))
        })
        .expect("workspace registered in Nearbee's apb instance")["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn load_state(ctx: &Ctx) -> Value {
    std::fs::read_to_string(&ctx.state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn save_state(ctx: &Ctx, v: &Value) {
    std::fs::write(&ctx.state_path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

async fn capture(ctx: &Ctx, name: &str) {
    let ws = workspace_id(ctx).await;
    let pb = ctx.playbook.clone();
    match name {
        "projects" => {
            exchange(ctx, name, "GET", "/api/projects", vec![], None).await;
        }
        "playbooks" => {
            exchange(ctx, name, "GET", "/api/playbooks", vec![], None).await;
        }
        "estimate" => {
            let path = format!("/api/playbooks/{pb}/estimate");
            exchange(ctx, name, "GET", &path, vec![("workspace", ws)], None).await;
        }
        "refusal_launch" => {
            // smoke-qwen binds an OpenCode profile whose billing basis apb
            // cannot tell; under the default `billing.unknown_basis: ask` the
            // start is refused before anything runs (no spend).
            let worktree = make_worktree(ctx, "refusal");
            let body = json!({
                "instruction": "Synthetic fixture capture; never runs.",
                "params": {"topic": "honey bees", "hire_id": "fixture-hire-refused", "execution_id": "fixture-exec-refused"},
                "worktree": worktree,
                "budget": {"usd": 0.5, "on_no_estimate": "ask"}
            });
            let path = "/api/playbooks/smoke-qwen/run";
            exchange(ctx, name, "POST", path, vec![("workspace", ws)], Some(body)).await;
        }
        "refusal_budget" => {
            // A budget that leaves `on_no_estimate` to the default (ask) on a
            // playbook without history: refused with 409 and the estimate
            // before anything runs. apb refuses only when the request leaves
            // the policy unset; with an explicit `ask` the run starts and
            // pauses before the first unestimated node instead.
            let worktree = make_worktree(ctx, "refusal");
            let body = json!({
                "instruction": "Synthetic fixture capture; never runs.",
                "params": {"topic": "honey bees", "hire_id": "fixture-hire-refused", "execution_id": "fixture-exec-refused"},
                "worktree": worktree,
                "budget": {"tokens": 200000, "usd": 0.5}
            });
            let path = format!("/api/playbooks/{pb}/run");
            exchange(
                ctx,
                name,
                "POST",
                &path,
                vec![("workspace", ws)],
                Some(body),
            )
            .await;
        }
        "refusal_invalid" => {
            let path = format!("/api/playbooks/{pb}/run");
            let body = json!({"instruction": "x", "params": {"topic": 3}});
            exchange(
                ctx,
                name,
                "POST",
                &path,
                vec![("workspace", ws)],
                Some(body),
            )
            .await;
        }
        "run_404" => {
            let path = "/api/runs/run-does-not-exist".to_string();
            exchange(ctx, name, "GET", &path, vec![("workspace", ws)], None).await;
        }
        "start" => {
            let suffix = std::process::id();
            let hire = format!("fixture-hire-{suffix}");
            let exec = format!("fixture-exec-{suffix}");
            let worktree = make_worktree(ctx, &format!("smoke-{suffix}"));
            let path = format!("/api/playbooks/{pb}/run");
            let body = json!({
                "instruction": "Synthetic smoke run for the Nearbee fixtures.",
                "params": {"topic": "honey bees", "hire_id": hire, "execution_id": exec},
                "worktree": worktree,
                "budget": {"usd": 0.5, "on_no_estimate": "continue"}
            });
            let resp = exchange(
                ctx,
                name,
                "POST",
                &path,
                vec![("workspace", ws.clone())],
                Some(body),
            )
            .await;
            let run_id = resp["run_id"]
                .as_str()
                .or_else(|| resp["id"].as_str())
                .expect("start answer carries a run id")
                .to_owned();
            let detail = exchange(
                ctx,
                "run_running",
                "GET",
                &format!("/api/runs/{run_id}"),
                vec![("workspace", ws)],
                None,
            )
            .await;
            if detail["run_status"] != "running" && detail["run_status"] != "created" {
                eprintln!(
                    "note: the run was already {} when read; run_running shows that state",
                    detail["run_status"]
                );
            }
            save_state(
                ctx,
                &json!({"run_id": run_id, "hire_id": hire, "execution_id": exec, "worktree": worktree}),
            );
        }
        "run_succeeded" => {
            let st = load_state(ctx);
            let run_id = st["run_id"].as_str().expect("run `start` first").to_owned();
            let deadline = Instant::now() + Duration::from_secs(15 * 60);
            loop {
                let v: Value = ctx
                    .http
                    .get(format!("{}/api/runs/{run_id}", ctx.url))
                    .query(&[("workspace", &ws)])
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                let s = v["run_status"].as_str().unwrap_or("").to_owned();
                if matches!(s.as_str(), "succeeded" | "failed" | "aborted") {
                    eprintln!("run {run_id} ended: {s}");
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "run {run_id} did not end in time"
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            exchange(
                ctx,
                name,
                "GET",
                &format!("/api/runs/{run_id}"),
                vec![("workspace", ws)],
                None,
            )
            .await;
        }
        "find_hire" | "find_execution" => {
            let st = load_state(ctx);
            let (field, key) = if name == "find_hire" {
                ("hire_id", "hire_id")
            } else {
                ("execution_id", "execution_id")
            };
            let value = st[key].as_str().expect("run `start` first");
            exchange(
                ctx,
                name,
                "GET",
                "/api/runs",
                vec![
                    ("workspace", ws),
                    ("playbook", pb),
                    ("field", format!("{field}:{value}")),
                ],
                None,
            )
            .await;
        }
        other => panic!("unknown capture `{other}`"),
    }
}

fn make_worktree(ctx: &Ctx, name: &str) -> String {
    let p = ctx.workspace_path.join("work").join(name);
    std::fs::create_dir_all(&p).unwrap();
    p.to_string_lossy().into_owned()
}

async fn exchange(
    ctx: &Ctx,
    name: &str,
    method: &str,
    path: &str,
    query: Vec<(&str, String)>,
    body: Option<Value>,
) -> Value {
    let url = format!("{}{}", ctx.url, path);
    let mut req = match method {
        "GET" => ctx.http.get(&url),
        "POST" => ctx.http.post(&url),
        m => panic!("method {m}"),
    };
    if !query.is_empty() {
        req = req.query(&query);
    }
    if let Some(b) = &body {
        req = req.json(b);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let headers: serde_json::Map<String, Value> = resp
        .headers()
        .iter()
        .filter(|(k, _)| matches!(k.as_str(), "content-type" | "retry-after"))
        .map(|(k, v)| (k.to_string(), json!(v.to_str().unwrap_or(""))))
        .collect();
    let text = resp.text().await.unwrap();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    let fixture = json!({
        "captured_at": now_rfc3339(),
        "apb_version": ctx.apb_version,
        "request": {
            "method": method,
            "path": path,
            "query": query.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "body": body,
        },
        "response": {"status": status, "headers": headers, "body": parsed},
    });
    let scrubbed = scrub(&serde_json::to_string_pretty(&fixture).unwrap(), &ctx.home);
    let file = ctx.out_dir.join(format!("{name}.json"));
    std::fs::write(&file, scrubbed + "\n").unwrap();
    eprintln!("captured {name}: {method} {path} -> {status}");
    parsed
}

fn scrub(text: &str, home: &str) -> String {
    text.replace(home, "<home>")
}

fn now_rfc3339() -> String {
    let out = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}
