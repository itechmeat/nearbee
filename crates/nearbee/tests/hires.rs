//! `nearbee hires` and `nearbee cleanup` over journals on disk (spec
//! `hire-retention`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use nearbee::{Cli, Env};
use nearbee_apb::Timing;
use nearbee_core::journal::{self, Event};
use time::OffsetDateTime;

struct Setup {
    dir: tempfile::TempDir,
    state: PathBuf,
    workspace: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(workspace.join("work")).unwrap();
        std::fs::write(
            dir.path().join("nearbee.yaml"),
            format!(
                "state_dir: {}\napb:\n  workspace_path: {}\nretention:\n  after_acceptance_days: 7\n",
                state.display(),
                workspace.display()
            ),
        )
        .unwrap();
        Self {
            dir,
            state,
            workspace,
        }
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("nearbee.yaml")
    }

    /// Writes a journal of `events`, each at its own time.
    fn journal(&self, hire: &str, events: Vec<(OffsetDateTime, u32, Event)>) {
        let mut text = String::new();
        for (i, (at, generation, e)) in events.into_iter().enumerate() {
            let r = journal::Record {
                schema_version: journal::SCHEMA_VERSION,
                seq: i as u64 + 1,
                timestamp: at,
                hire_id: hire.to_owned(),
                generation,
                event: e,
            };
            text.push_str(&r.to_line());
            text.push('\n');
        }
        let p = journal::journal_path(&self.state, hire);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A worktree and a snapshot directory with a file each.
    fn files(&self, hire: &str) -> (PathBuf, PathBuf) {
        let wt = self.workspace.join("work").join(journal::safe_id(hire));
        std::fs::create_dir_all(wt.join("out")).unwrap();
        std::fs::write(wt.join("out/deliverable.md"), "# x\n").unwrap();
        let snap = journal::gen_dir(&self.state, hire, 1);
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("submission.md"), "# x\n").unwrap();
        (wt, snap)
    }

    fn records(&self, hire: &str) -> Vec<journal::Record> {
        let text = std::fs::read_to_string(journal::journal_path(&self.state, hire)).unwrap();
        journal::read(&text).unwrap().records
    }
}

async fn run(s: &Setup, args: &[&str]) -> (i32, String) {
    let map: HashMap<String, String> = [("HOME".to_string(), "/home/u".to_string())].into();
    let var = move |k: &str| map.get(k).cloned();
    let env = Env {
        cwd: Path::new("/"),
        var: &var,
        timing: Timing {
            connect: Duration::from_millis(500),
            read: Duration::from_secs(2),
            estimate: Duration::from_secs(2),
            start: Duration::from_secs(2),
            abort: Duration::from_secs(5),
            decide: Duration::from_secs(5),
            read_retry_delays: vec![Duration::ZERO; 3],
        },
    };
    let cfg = s.config();
    let mut argv = vec!["nearbee", "--config", cfg.to_str().unwrap()];
    argv.extend_from_slice(args);
    let cli = Cli::try_parse_from(argv).unwrap();
    let mut out = Vec::new();
    let code = nearbee::run(cli, &env, &mut out).await;
    (code, String::from_utf8(out).unwrap())
}

const BRIEF: &str = "Audit the secret repository brief-text-marker.";

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn days_ago(d: i64) -> OffsetDateTime {
    now() - time::Duration::days(d)
}

fn received() -> Event {
    let mut r = journal::Received::from_brief(BRIEF);
    r.listing_id = Some("repo-audit-fast".into());
    Event::Received(r)
}

fn decision(d: &str) -> Event {
    Event::Decision(journal::Decision {
        decision: d.into(),
        reason: if d == "decline" {
            "rejected".into()
        } else {
            String::new()
        },
    })
}

fn lifecycle(status: &str) -> Event {
    Event::Lifecycle(journal::Lifecycle {
        status: status.into(),
        cancel_reason: None,
        finalized_at: None,
    })
}

fn run_intent(worktree: &Path) -> Event {
    Event::Intent(journal::Intent {
        operation_id: uuid::Uuid::now_v7(),
        kind: "run_start".into(),
        execution: 1,
        request_digest: "x".into(),
        refs: BTreeMap::from([
            (
                "worktree".to_owned(),
                worktree.to_string_lossy().to_string(),
            ),
            (
                "sla_deadline_at".to_owned(),
                "2026-11-11T12:00:00Z".to_owned(),
            ),
        ]),
    })
}

fn observed(run: &str, status: &str) -> Event {
    Event::RunObserved(journal::RunObserved {
        run_id: run.into(),
        execution: 1,
        status: status.into(),
        playbook_version: Some("1.0.1".into()),
        cost_usd: Some("0.05".into()),
        cost_complete: Some(true),
        budget_usd: Some("0.225".into()),
    })
}

fn op_result(intent: &Event) -> Event {
    match intent {
        Event::Intent(i) => Event::Result(journal::OpResult {
            operation_id: i.operation_id,
            outcome: "done".into(),
            external_ids: BTreeMap::new(),
        }),
        _ => unreachable!(),
    }
}

/// A delivered hire whose market side then reached `status`, `at`.
fn delivered_then(s: &Setup, hire: &str, status: &str, at: OffsetDateTime) {
    let wt = s.workspace.join("work").join(journal::safe_id(hire));
    let intent = run_intent(&wt);
    let t = at - time::Duration::hours(1);
    s.journal(
        hire,
        vec![
            (t, 1, received()),
            (t, 1, intent.clone()),
            (t, 1, observed("run-1", "succeeded")),
            (t, 1, op_result(&intent)),
            (t, 1, decision("delivered")),
            (at, 1, lifecycle(status)),
        ],
    );
}

// --------------------------------------------------------------------------
// nearbee hires (9.1)
// --------------------------------------------------------------------------

#[tokio::test]
async fn hires_lists_every_hire_with_attention_and_no_brief_text() {
    let s = Setup::new();
    delivered_then(&s, "h-accepted", "accepted", days_ago(1));
    let wt = s.workspace.join("work").join("h-open");
    s.journal(
        "h-open",
        vec![
            (now(), 1, received()),
            (now(), 1, run_intent(&wt)),
            (
                now(),
                1,
                Event::Attention(journal::Attention {
                    code: "start_unresolved".into(),
                    detail: "the start lookup was empty".into(),
                }),
            ),
        ],
    );
    // A disputed hire is frozen; a resolved dispute is closed.
    delivered_then(&s, "h-disputed", "disputed", days_ago(1));
    delivered_then(&s, "h-resolved", "dispute_resolved", days_ago(1));
    // A broken journal does not hide the others.
    let broken = journal::journal_path(&s.state, "h-broken");
    std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
    std::fs::write(&broken, "not json at all\n").unwrap();
    let (code, out) = run(&s, &["hires"]).await;
    assert_eq!(code, 0, "{out}");
    let line = |id: &str| {
        out.lines()
            .find(|l| l.starts_with(id))
            .unwrap_or_else(|| panic!("no line for {id} in {out}"))
            .to_owned()
    };
    let open = line("h-open");
    assert!(open.contains("open generation 1"), "{open}");
    assert!(open.ends_with("start_unresolved"), "{open}");
    assert!(open.contains("repo-audit-fast"), "{open}");
    assert!(open.contains("2026-11-11T12:00:00Z"), "{open}");
    let acc = line("h-accepted");
    assert!(
        acc.contains("closed accepted") && acc.contains("run-1") && acc.contains("0.05"),
        "{acc}"
    );
    assert!(
        line(&journal::safe_id("h-broken")).contains("unreadable"),
        "{out}"
    );
    assert!(line("h-disputed").contains("frozen"), "{out}");
    assert!(
        line("h-resolved").contains("closed dispute resolved"),
        "{out}"
    );
    assert!(
        !out.contains("brief-text-marker"),
        "the brief leaked: {out}"
    );
    let (code, json) = run(&s, &["hires", "--json"]).await;
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 5);
    assert!(!json.contains("brief-text-marker"));
    assert!(
        json.contains("\"attention\": \"start_unresolved\""),
        "{json}"
    );
}

// --------------------------------------------------------------------------
// nearbee cleanup (9.2)
// --------------------------------------------------------------------------

#[tokio::test]
async fn cleanup_follows_the_eligibility_rules() {
    let s = Setup::new();
    // Recently accepted: left.
    delivered_then(&s, "h-recent", "accepted", days_ago(2));
    let (recent_wt, _) = s.files("h-recent");
    // Declined at intake an hour ago: deleted now.
    s.journal(
        "h-intake",
        vec![
            (now(), 1, received()),
            (
                now(),
                1,
                Event::Intake(journal::Intake {
                    check: "hard_limits".into(),
                    outcome: "declined".into(),
                    reason: "over".into(),
                    within_first_hour: true,
                }),
            ),
            (now(), 1, decision("decline")),
        ],
    );
    let (intake_wt, intake_snap) = s.files("h-intake");
    // Cancelled after a delivery eight days ago: deleted.
    delivered_then(&s, "h-cancelled", "cancelled", days_ago(8));
    let (cancel_wt, _) = s.files("h-cancelled");
    // Disputed: never.
    delivered_then(&s, "h-disputed", "disputed", days_ago(30));
    let (disputed_wt, _) = s.files("h-disputed");
    // Accepted eight days ago (past the seven): deleted.
    delivered_then(&s, "h-old", "accepted", days_ago(8));
    let (old_wt, _) = s.files("h-old");
    // Cancelled with no delivery, just now: deleted.
    s.journal(
        "h-undelivered",
        vec![(now(), 1, received()), (now(), 1, lifecycle("cancelled"))],
    );
    let (undelivered_wt, _) = s.files("h-undelivered");
    // A dispute resolved eight days ago: deleted.
    delivered_then(&s, "h-resolved", "dispute_resolved", days_ago(8));
    let (resolved_wt, _) = s.files("h-resolved");
    // Cancelled after a delivery two days ago (inside the window): left.
    delivered_then(&s, "h-cancel-recent", "cancelled", days_ago(2));
    let (cancel_recent_wt, _) = s.files("h-cancel-recent");
    let (code, out) = run(&s, &["cleanup"]).await;
    assert_eq!(code, 0, "{out}");
    assert!(recent_wt.exists(), "{out}");
    assert!(disputed_wt.exists(), "{out}");
    assert!(cancel_recent_wt.exists(), "{out}");
    assert!(!intake_wt.exists() && !intake_snap.exists(), "{out}");
    assert!(!cancel_wt.exists(), "{out}");
    assert!(!old_wt.exists(), "{out}");
    assert!(!undelivered_wt.exists(), "{out}");
    assert!(!resolved_wt.exists(), "{out}");
    assert!(
        out.contains("not covered: apb run directories of run-1"),
        "{out}"
    );
    // The deleted record lists the inventory; the journal stays.
    let rec = s.records("h-intake");
    let deleted = rec
        .iter()
        .find_map(|r| match &r.event {
            Event::Deleted(d) => Some(d.what.clone()),
            _ => None,
        })
        .expect("a deleted record");
    assert!(
        deleted
            .iter()
            .any(|w| w.starts_with("removed ") && w.contains("work")),
        "{deleted:?}"
    );
    assert!(
        deleted.iter().any(|w| w.starts_with("not covered: ")),
        "{deleted:?}"
    );
    // hires shows it as deleted; a second cleanup leaves it.
    let (_, listing) = run(&s, &["hires"]).await;
    assert!(
        listing
            .lines()
            .any(|l| l.starts_with("h-intake") && l.contains("\tdeleted\t")),
        "{listing}"
    );
    let (_, again) = run(&s, &["cleanup"]).await;
    assert!(again.contains("keep h-intake: already deleted"), "{again}");
}

#[tokio::test]
async fn a_worktree_outside_work_is_refused_and_never_touched() {
    let s = Setup::new();
    let outside = s.dir.path().join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "x").unwrap();
    let intent = run_intent(&outside);
    s.journal(
        "h-out",
        vec![
            (now(), 1, received()),
            (now(), 1, intent.clone()),
            (now(), 1, observed("run-9", "failed")),
            (now(), 1, op_result(&intent)),
            (now(), 1, decision("decline")),
        ],
    );
    // A `..` path that climbs out of work/ is refused too.
    let climbed = s.workspace.join("climbed");
    std::fs::create_dir_all(&climbed).unwrap();
    std::fs::write(climbed.join("keep.txt"), "x").unwrap();
    let climb = run_intent(&s.workspace.join("work").join("..").join("climbed"));
    s.journal(
        "h-climb",
        vec![
            (now(), 1, received()),
            (now(), 1, climb.clone()),
            (now(), 1, observed("run-8", "failed")),
            (now(), 1, op_result(&climb)),
            (now(), 1, decision("decline")),
        ],
    );
    let (code, out) = run(&s, &["cleanup"]).await;
    assert_eq!(code, 0, "{out}");
    assert!(outside.join("keep.txt").exists());
    assert!(climbed.join("keep.txt").exists(), "{out}");
    assert!(
        out.contains("refused") && out.contains("elsewhere"),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.contains("refused") && l.contains("climbed")),
        "{out}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_partial_cleanup_is_recorded_and_retried() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    s.journal(
        "h-part",
        vec![(now(), 1, received()), (now(), 1, decision("decline"))],
    );
    let (wt, snap) = s.files("h-part");
    // The snapshot's contents cannot be removed.
    let locked = snap.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("f"), "x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::remove_file(locked.join("f")).is_ok() {
        // Rights that ignore the mode (root): not exercisable here.
        eprintln!("skipped: the process ignores directory modes (root)");
        return;
    }
    let (code, out) = run(&s, &["cleanup"]).await;
    assert_eq!(code, 1, "{out}");
    assert!(!wt.exists());
    let rec = s.records("h-part");
    let what = rec
        .iter()
        .find_map(|r| match &r.event {
            Event::Deleted(d) => Some(d.what.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        what.iter()
            .any(|w| w.starts_with("removed ") && w.contains("work")),
        "{what:?}"
    );
    assert!(
        what.iter()
            .any(|w| w.starts_with("failed ") && w.contains("gen-1")),
        "{what:?}"
    );
    // Still eligible: the next cleanup retries and finishes.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (code, out) = run(&s, &["cleanup"]).await;
    assert_eq!(code, 0, "{out}");
    assert!(!snap.exists());
    let deleted_records = s
        .records("h-part")
        .iter()
        .filter(|r| matches!(r.event, Event::Deleted(_)))
        .count();
    assert_eq!(deleted_records, 2);
}

#[tokio::test]
async fn a_dry_run_changes_nothing_and_takes_no_lock() {
    let s = Setup::new();
    s.journal(
        "h-dry",
        vec![(now(), 1, received()), (now(), 1, decision("decline"))],
    );
    let (wt, snap) = s.files("h-dry");
    let before = std::fs::read_to_string(journal::journal_path(&s.state, "h-dry")).unwrap();
    // A held lock does not stop the dry run.
    let _held = nearbee_core::lock::acquire(&s.state).unwrap();
    let (code, out) = run(&s, &["cleanup", "--dry-run"]).await;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("would delete h-dry"), "{out}");
    assert!(out.contains(&wt.display().to_string()), "{out}");
    assert!(wt.exists() && snap.exists());
    let after = std::fs::read_to_string(journal::journal_path(&s.state, "h-dry")).unwrap();
    assert_eq!(before, after);
    // The real cleanup needs the lock: exit 4 while it is held.
    let (code, out) = run(&s, &["cleanup"]).await;
    assert_eq!(code, 4, "{out}");
    assert!(wt.exists());
}
