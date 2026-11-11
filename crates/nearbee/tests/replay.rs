//! `nearbee replay` (change `judges-path-and-submission`, design 1, spec
//! `replay-walkthrough`), running the built binary.

use std::path::Path;
use std::process::{Command, Output};

/// The binary with an empty home, an empty config home, a `PATH` without
/// apb and a private temporary directory.
fn replay(args: &[&str], home: &Path) -> Output {
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    Command::new(env!("CARGO_BIN_EXE_nearbee"))
        .arg("replay")
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", &tmp)
        .current_dir(home)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

/// True when `s` holds a UUID (8-4-4-4-12 hex digits).
fn has_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    let groups = [8, 4, 4, 4, 12];
    let len: usize = groups.iter().sum::<usize>() + 4;
    (0..b.len().saturating_sub(len - 1)).any(|start| {
        let mut i = start;
        for (n, g) in groups.iter().enumerate() {
            if !b[i..i + g].iter().all(u8::is_ascii_hexdigit) {
                return false;
            }
            i += g;
            if n < 4 {
                if b[i] != b'-' {
                    return false;
                }
                i += 1;
            }
        }
        true
    })
}

#[test]
fn every_scenario_passes_on_a_clean_machine() {
    let home = tempfile::tempdir().unwrap();
    let o = replay(&[], home.path());
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    let first = out.lines().next().unwrap();
    assert!(
        first.starts_with("Nearbee replay: a scripted walkthrough")
            && first.contains("contacts nothing"),
        "{first}"
    );
    let banner: String = out.lines().take(4).collect::<Vec<_>>().join("\n");
    assert!(
        banner.contains("synthetic") && banner.contains("will not verify"),
        "{banner}"
    );
    for name in ["success", "decline", "redo", "restart", "cancellation"] {
        assert!(
            out.contains(&format!("scenario {name}: ok")),
            "{name}: {out}"
        );
    }
    assert!(out.contains("replay: 5 of 5 scenarios ok"), "{out}");
    assert!(!has_uuid(&out), "the output holds a uuid: {out}");
    assert!(
        !out.contains(&home.path().to_string_lossy().into_owned()),
        "a path leaked: {out}"
    );
    // The temporary state is gone and nothing was written to the home.
    assert_eq!(
        std::fs::read_dir(home.path().join("tmp")).unwrap().count(),
        0
    );
    assert!(!home.path().join("config").exists());
}

#[test]
fn one_scenario_runs_alone_and_shows_the_lookup() {
    let home = tempfile::tempdir().unwrap();
    let o = replay(&["--scenario", "restart"], home.path());
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("scenario restart: ok"), "{out}");
    assert!(!out.contains("scenario success"), "{out}");
    assert!(out.contains("answer lost"), "{out}");
    assert!(
        out.contains("lookup by") && out.contains("found 1 run"),
        "{out}"
    );
    assert!(out.contains("replay: 1 of 1 scenarios ok"), "{out}");
}

#[test]
fn an_unknown_scenario_exits_2_naming_the_known_ones() {
    let home = tempfile::tempdir().unwrap();
    let o = replay(&["--scenario", "nope"], home.path());
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(2), "{out}");
    assert!(
        out.contains("success, decline, redo, restart, cancellation"),
        "{out}"
    );
    assert!(
        !out.contains("Nearbee replay:") && !out.contains(": ok"),
        "something ran: {out}"
    );
}

#[test]
fn a_non_empty_keep_directory_exits_2_and_an_empty_one_is_kept() {
    let home = tempfile::tempdir().unwrap();
    let full = home.path().join("full");
    std::fs::create_dir_all(&full).unwrap();
    std::fs::write(full.join("x"), "x").unwrap();
    let o = replay(&["--keep", full.to_str().unwrap()], home.path());
    assert_eq!(o.status.code(), Some(2), "{}", stdout(&o));
    let kept = home.path().join("kept");
    let o = replay(
        &["--scenario", "decline", "--keep", kept.to_str().unwrap()],
        home.path(),
    );
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert!(kept.join("decline").is_dir());
}

#[test]
fn verbose_names_the_journal_events() {
    let home = tempfile::tempdir().unwrap();
    let o = replay(&["--scenario", "decline", "--verbose"], home.path());
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("[journal: received"), "{out}");
    assert!(out.contains("intent market_decline"), "{out}");
}
