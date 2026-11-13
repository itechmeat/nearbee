//! Agent A's tools (tasks 8.1 to 8.3): the archive reader with tarballs
//! built here, `fetch_snapshot` against `wiremock`, and `finish`.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use nearbee::audit::{self, Source};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn source() -> Source {
    Source {
        url: "https://github.com/octo/demo".into(),
        owner: "octo".into(),
        repo: "demo".into(),
        sha: SHA.into(),
    }
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(120)
}

/// A tar member: a raw header whose name is written as given (so `..`
/// and absolute names reach the reader), with `data`.
fn raw_member(
    b: &mut tar::Builder<impl Write>,
    name: &str,
    kind: tar::EntryType,
    link: Option<&str>,
    data: &[u8],
) {
    let mut h = tar::Header::new_gnu();
    {
        let old = h.as_old_mut();
        old.name[..name.len()].copy_from_slice(name.as_bytes());
        if let Some(l) = link {
            old.linkname[..l.len()].copy_from_slice(l.as_bytes());
        }
    }
    h.set_entry_type(kind);
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    b.append(&h, data).unwrap();
}

fn file(b: &mut tar::Builder<impl Write>, name: &str, data: &[u8]) {
    raw_member(b, name, tar::EntryType::Regular, None, data);
}

/// Gzips the tar the closure writes.
fn tarball(fill: impl FnOnce(&mut tar::Builder<Vec<u8>>)) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    fill(&mut b);
    let tar = b.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn coverage(dir: &Path) -> audit::Coverage {
    serde_json::from_str(&read(dir, "audit-snapshot/coverage.json")).unwrap()
}

fn nothing_published(dir: &Path) {
    assert!(
        !dir.join(audit::SNAPSHOT_DIR).exists(),
        "a snapshot was published"
    );
    assert!(!dir.join(audit::STAGING_DIR).exists(), "staging was left");
}

// ---------------------------------------------------------------------------
// 8.1 The archive reader
// ---------------------------------------------------------------------------

#[test]
fn files_land_under_repo_without_the_top_directory() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        raw_member(b, "octo-demo-0123/", tar::EntryType::Directory, None, b"");
        file(b, "octo-demo-0123/README.md", b"# Demo\n");
        file(b, "octo-demo-0123/src/main.rs", b"fn main() {}\n");
    });
    let cov = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    assert_eq!(
        read(dir.path(), "audit-snapshot/repo/README.md"),
        "# Demo\n"
    );
    assert_eq!(
        read(dir.path(), "audit-snapshot/repo/src/main.rs"),
        "fn main() {}\n"
    );
    assert!(
        !dir.path()
            .join("audit-snapshot/repo/octo-demo-0123")
            .exists()
    );
    assert_eq!(cov.files_written, 2);
    let src: audit::SourceRecord =
        serde_json::from_str(&read(dir.path(), "audit-snapshot/source.json")).unwrap();
    assert_eq!(src.version, 1);
    assert_eq!(src.sha, SHA);
    assert_eq!(src.url, "https://github.com/octo/demo");
    let paths: Vec<&str> = src.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["README.md", "src/main.rs"]);
    assert_eq!(src.files[0].size, 7);
    assert!(!dir.path().join(audit::STAGING_DIR).exists());
}

#[test]
fn links_traversal_and_absolute_members_are_skipped_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        raw_member(
            b,
            "top/passwd",
            tar::EntryType::Symlink,
            Some("/etc/passwd"),
            b"",
        );
        raw_member(
            b,
            "top/hard",
            tar::EntryType::Link,
            Some("top/README.md"),
            b"",
        );
        file(b, "top/../escape.txt", b"x");
        file(b, "/abs.txt", b"x");
        raw_member(b, "top/fifo", tar::EntryType::Fifo, None, b"");
        file(b, "top/README.md", b"ok\n");
    });
    let cov = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    assert_eq!(cov.skipped.get("link"), Some(&2));
    assert_eq!(cov.skipped.get("unsafe_path"), Some(&2));
    assert_eq!(cov.skipped.get("special"), Some(&1));
    assert_eq!(cov.files_written, 1);
    assert!(std::fs::symlink_metadata(dir.path().join("audit-snapshot/repo/passwd")).is_err());
    assert!(!dir.path().join("escape.txt").exists());
    assert!(!dir.path().join("audit-snapshot/escape.txt").exists());
    assert_eq!(coverage(dir.path()), cov);
}

#[test]
fn a_two_mebibyte_file_and_a_binary_file_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let big = vec![b'a'; 2 * 1024 * 1024];
    let mut binary = b"text".to_vec();
    binary.push(0);
    let gz = tarball(|b| {
        file(b, "top/big.txt", &big);
        file(b, "top/blob.bin", &binary);
        file(b, "top/ok.txt", b"ok");
    });
    let cov = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    assert_eq!(cov.skipped.get("too_large"), Some(&1));
    assert_eq!(cov.skipped.get("binary"), Some(&1));
    assert!(!dir.path().join("audit-snapshot/repo/big.txt").exists());
    assert!(!dir.path().join("audit-snapshot/repo/blob.bin").exists());
    assert!(dir.path().join("audit-snapshot/repo/ok.txt").exists());
}

#[test]
fn skipped_data_counts_against_the_decompressed_cap() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_path("top/zeros.bin").unwrap();
    let size = audit::MAX_DECOMPRESSED + 1024 * 1024;
    h.set_size(size);
    h.set_mode(0o644);
    h.set_cksum();
    b.append(&h, std::io::repeat(0).take_bytes(size)).unwrap();
    let tar = b.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    let gz = gz.finish().unwrap();
    let err = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap_err();
    assert!(err.contains("decompresses past"), "{err}");
    nothing_published(dir.path());
}

/// `Read::take` without importing the trait at every call.
trait TakeBytes: std::io::Read + Sized {
    fn take_bytes(self, n: u64) -> std::io::Take<Self> {
        std::io::Read::take(self, n)
    }
}
impl<R: std::io::Read> TakeBytes for R {}

#[test]
fn the_member_cap_fails() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        for i in 0..=audit::MAX_MEMBERS {
            file(b, &format!("top/f{i}.txt"), b"");
        }
    });
    let err = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap_err();
    assert!(err.contains("more than 20000 members"), "{err}");
    nothing_published(dir.path());
}

#[test]
fn a_truncated_gzip_fails_and_publishes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        file(b, "top/a.txt", &vec![b'x'; 64 * 1024]);
        file(b, "top/b.txt", b"b");
    });
    let cut = &gz[..gz.len() / 2];
    assert!(audit::build_snapshot(dir.path(), cut, &source(), later()).is_err());
    nothing_published(dir.path());
}

/// A stream cut after the tar end marker, or with a corrupt trailer,
/// fails: the gzip CRC and length trailer is read and checked (spec
/// `repo-audit`, Archive limits: truncated data fails).
#[test]
fn a_gzip_stream_without_its_trailer_or_with_a_bad_crc_fails() {
    let gz = tarball(|b| file(b, "top/a.txt", b"a\n"));
    let dir = tempfile::tempdir().unwrap();
    let cut = &gz[..gz.len() - 8];
    let err = audit::build_snapshot(dir.path(), cut, &source(), later()).unwrap_err();
    assert!(err.contains("cannot be read"), "{err}");
    nothing_published(dir.path());
    let mut bad = gz.clone();
    let crc = bad.len() - 8;
    bad[crc] ^= 0xff;
    let err = audit::build_snapshot(dir.path(), &bad, &source(), later()).unwrap_err();
    assert!(err.contains("cannot be read"), "{err}");
    nothing_published(dir.path());
    // The whole stream still publishes.
    audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
}

#[test]
fn a_duplicate_path_and_a_rename_collision_fail() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        file(b, "top/a.txt", b"1");
        file(b, "top/a.txt", b"2");
    });
    let err = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap_err();
    assert!(err.contains("duplicate or colliding"), "{err}");
    nothing_published(dir.path());

    let gz = tarball(|b| {
        file(b, "top/CLAUDE.md", b"1");
        file(b, "top/CLAUDE.md.repo-data", b"2");
    });
    let err = audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap_err();
    assert!(err.contains("duplicate or colliding"), "{err}");
    nothing_published(dir.path());
}

#[test]
fn instruction_files_are_renamed_at_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        file(b, "top/AGENTS.md", b"a");
        file(b, "top/src/CLAUDE.md", b"c");
        file(b, "top/docs/GEMINI.md", b"g");
        file(b, "top/opencode.json", b"{}");
        file(b, "top/.claude/settings.json", b"{}");
        file(b, "top/pkg/.cursor/rules.md", b"r");
        file(b, "top/.github/copilot-instructions.md", b"p");
        file(b, "top/.github/workflows/ci.yml", b"w");
    });
    audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    let repo = dir.path().join("audit-snapshot/repo");
    for renamed in [
        "AGENTS.md.repo-data",
        "src/CLAUDE.md.repo-data",
        "docs/GEMINI.md.repo-data",
        "opencode.json.repo-data",
        ".claude.repo-data/settings.json",
        "pkg/.cursor.repo-data/rules.md",
        ".github/copilot-instructions.md.repo-data",
        ".github/workflows/ci.yml",
    ] {
        assert!(repo.join(renamed).is_file(), "{renamed} missing");
    }
    for original in [
        "AGENTS.md",
        "src/CLAUDE.md",
        ".claude",
        ".github/copilot-instructions.md",
    ] {
        assert!(!repo.join(original).exists(), "{original} was written");
    }
}

#[test]
fn a_past_deadline_fails_and_publishes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| file(b, "top/a.txt", b"a"));
    let err = audit::build_snapshot(dir.path(), &gz, &source(), Instant::now()).unwrap_err();
    assert!(err.contains("deadline"), "{err}");
    nothing_published(dir.path());
}

#[cfg(unix)]
#[test]
fn a_leftover_staging_directory_or_link_is_removed_and_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    // A leftover staging directory with stale content.
    std::fs::create_dir_all(dir.path().join("audit-snapshot.partial/repo")).unwrap();
    std::fs::write(
        dir.path().join("audit-snapshot.partial/repo/stale.txt"),
        b"old",
    )
    .unwrap();
    let gz = tarball(|b| file(b, "top/a.txt", b"a"));
    audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    assert!(!dir.path().join("audit-snapshot/repo/stale.txt").exists());
    assert!(dir.path().join("audit-snapshot/repo/a.txt").exists());

    // A link in its place pointing at a directory outside: the link goes,
    // the target and its content stay.
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("audit-snapshot.partial")).unwrap();
    audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    assert!(
        outside.path().join("keep.txt").exists(),
        "the link was followed"
    );
    assert!(
        !outside.path().join("repo").exists(),
        "wrote through the link"
    );
    assert!(dir.path().join("audit-snapshot/repo/a.txt").exists());
}

// ---------------------------------------------------------------------------
// 8.2 fetch_snapshot against wiremock
// ---------------------------------------------------------------------------

const BRIEF: &str = "Please audit https://github.com/octo/demo for problems.";

async fn github(private: bool) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/octo/demo"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"private": private, "default_branch": "main"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo/demo/commits/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"sha": SHA})))
        .mount(&server)
        .await;
    let gz = tarball(|b| file(b, "octo-demo-0123/src/lib.rs", b"pub fn f() {}\n"));
    Mock::given(method("GET"))
        .and(path(format!("/octo/demo/tar.gz/{SHA}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(gz))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn the_commit_sha_lands_in_source_json() {
    let server = github(false).await;
    let dir = tempfile::tempdir().unwrap();
    let got = audit::fetch_snapshot(&server.uri(), &server.uri(), BRIEF, dir.path())
        .await
        .unwrap();
    assert!(
        matches!(got, audit::Fetched::Published { ref sha, .. } if sha == SHA),
        "{got:?}"
    );
    let src: audit::SourceRecord =
        serde_json::from_str(&read(dir.path(), "audit-snapshot/source.json")).unwrap();
    assert_eq!(src.sha, SHA);
    assert_eq!(src.owner, "octo");
    assert_eq!(
        read(dir.path(), "audit-snapshot/repo/src/lib.rs"),
        "pub fn f() {}\n"
    );
}

#[tokio::test]
async fn a_private_repository_is_refused() {
    let server = github(true).await;
    let dir = tempfile::tempdir().unwrap();
    let err = audit::fetch_snapshot(&server.uri(), &server.uri(), BRIEF, dir.path())
        .await
        .unwrap_err();
    assert!(err.contains("not public"), "{err}");
    nothing_published(dir.path());
}

#[tokio::test]
async fn a_redirect_is_refused_and_not_followed() {
    let server = MockServer::start().await;
    Mock::given(path("/repos/octo/demo"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("location", format!("{}/elsewhere", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(path("/elsewhere"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let err = audit::fetch_snapshot(&server.uri(), &server.uri(), BRIEF, dir.path())
        .await
        .unwrap_err();
    assert!(err.contains("301"), "{err}");
    nothing_published(dir.path());
}

#[tokio::test]
async fn a_published_snapshot_of_the_same_repository_is_reused_without_a_request() {
    let first = github(false).await;
    let dir = tempfile::tempdir().unwrap();
    audit::fetch_snapshot(&first.uri(), &first.uri(), BRIEF, dir.path())
        .await
        .unwrap();
    let silent = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&silent)
        .await;
    // The same repository, named another way.
    let got = audit::fetch_snapshot(
        &silent.uri(),
        &silent.uri(),
        "rework of https://github.com/Octo/Demo.git please",
        dir.path(),
    )
    .await
    .unwrap();
    assert_eq!(got, audit::Fetched::Reused { sha: SHA.into() });
}

#[tokio::test]
async fn a_brief_with_two_repositories_fails() {
    let silent = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&silent)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let err = audit::fetch_snapshot(
        &silent.uri(),
        &silent.uri(),
        "https://github.com/a/b and https://github.com/c/d",
        dir.path(),
    )
    .await
    .unwrap_err();
    assert!(err.contains("2 repositories named"), "{err}");
}

// ---------------------------------------------------------------------------
// 8.3 finish
// ---------------------------------------------------------------------------

const LIB: &str =
    "use std::fs;\n\nfn main() {\n    let key = \"sk-live-123\";\n    println!(\"{}\", key);\n}\n";

/// A worktree with a published snapshot of one 6-line file.
fn snapshot_worktree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| file(b, "top/src/main.rs", LIB.as_bytes()));
    audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    std::fs::create_dir_all(dir.path().join("audit")).unwrap();
    dir
}

fn finding(title: &str, file: &str, start: u64, end: u64, excerpt: &str) -> serde_json::Value {
    serde_json::json!({
        "severity": "high",
        "title": title,
        "file": file,
        "line_start": start,
        "line_end": end,
        "excerpt": excerpt,
        "detail": "A secret is hard-coded.",
        "fix": "Read it from the environment."
    })
}

fn write_candidates(dir: &Path, findings: Vec<serde_json::Value>) {
    let doc = serde_json::json!({"version": 1, "findings": findings});
    std::fs::write(dir.join("audit/candidates.json"), doc.to_string()).unwrap();
}

#[test]
fn a_grounded_finding_is_accepted_and_the_result_validates() {
    let dir = snapshot_worktree();
    write_candidates(
        dir.path(),
        vec![finding(
            "Hard-coded key",
            "src/main.rs",
            3,
            5,
            "  let key = \"sk-live-123\";\n\n println!(\"{}\", key);",
        )],
    );
    let f = audit::finish(dir.path(), None, &[]).unwrap();
    assert_eq!(f.accepted.len(), 1);
    assert!(f.rejected.is_empty());
    let validated =
        nearbee_core::manifest::validate(dir.path(), &nearbee_core::config::Limits::default())
            .unwrap();
    assert!(validated.files.is_empty());
    assert!(validated.markdown_text.contains("Hard-coded key"));
    assert!(validated.markdown_text.contains("location checked"));
    let json: audit::Findings =
        serde_json::from_str(&read(dir.path(), "out/findings.json")).unwrap();
    assert_eq!(json.repository.sha, SHA);
    assert_eq!(json, f);
}

/// A snapshot file the agent step rewrote grounds nothing (spec
/// `repo-audit`, Grounding check): fetch recorded its SHA-256, and a
/// finding citing a line only the rewrite added is rejected with `file
/// changed after fetch`, while a finding on an untouched file passes.
#[test]
fn a_finding_on_a_file_changed_after_fetch_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let gz = tarball(|b| {
        file(b, "top/src/main.rs", LIB.as_bytes());
        file(b, "top/src/ok.rs", b"fn ok() {}\n");
    });
    audit::build_snapshot(dir.path(), &gz, &source(), later()).unwrap();
    std::fs::create_dir_all(dir.path().join("audit")).unwrap();
    let recorded: audit::SourceRecord =
        serde_json::from_str(&read(dir.path(), "audit-snapshot/source.json")).unwrap();
    let main = recorded
        .files
        .iter()
        .find(|f| f.path == "src/main.rs")
        .unwrap();
    assert_eq!(
        main.sha256,
        nearbee_core::journal::sha256_hex(LIB.as_bytes())
    );
    std::fs::write(
        dir.path().join("audit-snapshot/repo/src/main.rs"),
        format!("{LIB}let password = \"hunter2\";\n"),
    )
    .unwrap();
    write_candidates(
        dir.path(),
        vec![
            finding(
                "Invented",
                "src/main.rs",
                7,
                7,
                "let password = \"hunter2\";",
            ),
            finding("Untouched", "src/ok.rs", 1, 1, "fn ok() {}"),
        ],
    );
    let f = audit::finish(dir.path(), None, &[]).unwrap();
    assert_eq!(f.accepted.len(), 1);
    assert_eq!(f.accepted[0].title, "Untouched");
    assert_eq!(f.rejected.len(), 1);
    assert_eq!(f.rejected[0].reason, "file changed after fetch");
}

#[test]
fn ungrounded_findings_are_rejected_with_reasons() {
    let dir = snapshot_worktree();
    write_candidates(
        dir.path(),
        vec![
            finding("Unknown file", "src/other.rs", 1, 1, "x"),
            finding("Invented line", "src/main.rs", 900, 900, "x"),
            finding("Empty excerpt", "src/main.rs", 1, 1, " \n  \n"),
            finding(
                "Not contiguous",
                "src/main.rs",
                1,
                5,
                "use std::fs;\nprintln!(\"{}\", key);",
            ),
            finding("Outside the lines", "src/main.rs", 1, 2, "fn main() {"),
        ],
    );
    let f = audit::finish(dir.path(), None, &[]).unwrap();
    assert!(f.accepted.is_empty());
    let reasons: Vec<&str> = f.rejected.iter().map(|r| r.reason.as_str()).collect();
    assert_eq!(
        reasons,
        vec![
            "unknown file",
            "line out of range",
            "empty excerpt",
            "excerpt not contiguous in the cited lines",
            "excerpt not contiguous in the cited lines",
        ]
    );
    assert!(dir.path().join("out/result.json").exists());
}

fn fails_without_result(dir: &Path, candidates: &[u8]) -> String {
    std::fs::write(dir.join("audit/candidates.json"), candidates).unwrap();
    let err = audit::finish(dir, None, &[]).unwrap_err();
    assert!(
        !dir.join("out/result.json").exists(),
        "out/result.json remains"
    );
    err
}

#[test]
fn a_wrong_version_an_unknown_field_or_invalid_json_fail_with_no_result() {
    let dir = snapshot_worktree();
    let err = fails_without_result(dir.path(), br#"{"version":2,"findings":[]}"#);
    assert!(err.contains("version 2"), "{err}");
    let err = fails_without_result(dir.path(), br#"{"version":1,"findings":[],"extra":1}"#);
    assert!(err.contains("unknown field"), "{err}");
    let mut f = finding("t", "src/main.rs", 1, 1, "use std::fs;");
    f["confidence"] = serde_json::json!(0.9);
    let doc = serde_json::json!({"version": 1, "findings": [f]});
    let err = fails_without_result(dir.path(), doc.to_string().as_bytes());
    assert!(err.contains("unknown field"), "{err}");
    fails_without_result(dir.path(), b"not json");
}

#[test]
fn an_oversized_candidates_file_fails_with_no_result() {
    let dir = snapshot_worktree();
    let big = vec![b' '; (audit::MAX_INPUT + 1) as usize];
    let err = fails_without_result(dir.path(), &big);
    assert!(err.contains("larger than"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_linked_candidates_file_fails_with_no_result() {
    let dir = snapshot_worktree();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        outside.path().join("c.json"),
        br#"{"version":1,"findings":[]}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("c.json"),
        dir.path().join("audit/candidates.json"),
    )
    .unwrap();
    let err = audit::finish(dir.path(), None, &[]).unwrap_err();
    assert!(err.contains("symbolic link"), "{err}");
    assert!(!dir.path().join("out/result.json").exists());
}

#[test]
fn a_failed_finish_after_a_successful_one_leaves_no_result() {
    let dir = snapshot_worktree();
    write_candidates(dir.path(), Vec::new());
    audit::finish(dir.path(), None, &[]).unwrap();
    assert!(dir.path().join("out/result.json").exists());
    fails_without_result(dir.path(), b"{}");
}

#[test]
fn finding_text_is_escaped_and_the_json_fence_stays_closed() {
    let dir = snapshot_worktree();
    let mut f = finding(
        "</details> ```` fence [x](y) *b* # h | t & _u_",
        "src/main.rs",
        1,
        1,
        "use std::fs;",
    );
    f["detail"] = serde_json::json!("- list\n+ plus\n12. numbered\nplain");
    f["fix"] = serde_json::json!("before\n~~~\n  ~~~~ rust\nafter ~~~ inline");
    write_candidates(dir.path(), vec![f]);
    audit::finish(dir.path(), None, &[]).unwrap();
    let md = read(dir.path(), "out/deliverable.md");
    assert!(md.contains("&lt;/details&gt;"), "{md}");
    // Raw only inside the JSON fence, where it is text.
    let prose = md.split("## Findings JSON").next().unwrap();
    assert!(!prose.contains("</details>"), "{md}");
    assert!(
        md.contains("\\`\\`\\`\\` fence \\[x\\](y) \\*b\\* \\# h \\| t &amp; \\_u\\_"),
        "{md}"
    );
    assert!(
        md.contains("\n\\- list\n\\+ plus\n12\\. numbered\nplain"),
        "{md}"
    );
    // A tilde fence at a line start (indented too) is escaped, so it
    // opens no code block that would swallow the rest; a run inside a
    // line is left alone.
    assert!(
        md.contains("Fix: before\n\\~~~\n  \\~~~~ rust\nafter ~~~ inline\n"),
        "{md}"
    );
    assert!(
        !md.lines().any(|l| l.trim_start().starts_with("~~~")),
        "{md}"
    );
    // The JSON holds a run of four backticks: the fence has five.
    assert!(md.contains("\n`````json\n"), "{md}");
    assert!(md.trim_end().ends_with("\n`````"), "{md}");
    assert_eq!(md.matches("`````").count(), 2, "{md}");
}

#[test]
fn json_without_backticks_gets_a_three_backtick_fence() {
    assert_eq!(audit::fence_for("{\"a\":1}"), "```");
    assert_eq!(audit::fence_for("a ``` b"), "````");
    let dir = snapshot_worktree();
    write_candidates(dir.path(), Vec::new());
    audit::finish(dir.path(), None, &[]).unwrap();
    let md = read(dir.path(), "out/deliverable.md");
    assert!(md.contains("\n```json\n"), "{md}");
}

/// Writes a candidates file with one grounded finding and `notes` (none
/// when `None`).
fn write_candidates_with_notes(dir: &Path, notes: Option<serde_json::Value>) {
    let mut doc = serde_json::json!({
        "version": 1,
        "findings": [finding("Hard-coded key", "src/main.rs", 4, 4, "    let key = \"sk-live-123\";")],
    });
    if let Some(n) = notes {
        doc["notes"] = n;
    }
    std::fs::write(dir.join("audit/candidates.json"), doc.to_string()).unwrap();
}

/// SHA-256 of the deliverable of `write_candidates_with_notes(dir, None)`
/// before review notes existed (printed on main before the
/// `sandbox-toolchains` change).
const DELIVERABLE_WITHOUT_NOTES_SHA256: &str =
    "a230902ed24b16bc7af414e0f825554009711421fcd495d920cff3f3a0d44ba0";

/// Spec `repo-audit`, Review notes in the deliverable: absent or empty
/// notes leave the deliverable byte for byte as before.
#[test]
fn absent_or_empty_notes_leave_the_deliverable_unchanged() {
    let dir = snapshot_worktree();
    write_candidates_with_notes(dir.path(), None);
    audit::finish(dir.path(), None, &[]).unwrap();
    let before = read(dir.path(), "out/deliverable.md");
    assert_eq!(
        nearbee_core::journal::sha256_hex(before.as_bytes()),
        DELIVERABLE_WITHOUT_NOTES_SHA256,
        "{before}"
    );
    write_candidates_with_notes(dir.path(), Some(serde_json::json!([])));
    audit::finish(dir.path(), None, &[]).unwrap();
    assert_eq!(read(dir.path(), "out/deliverable.md"), before);
    assert!(!before.contains("Review notes"), "{before}");
}

/// Spec `repo-audit`, Failed lens reported: the notes are rendered in
/// order, escaped, under "Review notes" before "Not reported".
#[test]
fn notes_are_rendered_in_order_and_escaped() {
    let dir = snapshot_worktree();
    write_candidates_with_notes(
        dir.path(),
        Some(serde_json::json!([
            "The tests lens failed after one send-back",
            "- 2 findings </details> *dropped* by the verifier\n# not a heading",
        ])),
    );
    audit::finish(dir.path(), None, &[]).unwrap();
    let md = read(dir.path(), "out/deliverable.md");
    let expected = "\n## Review notes\n\n- The tests lens failed after one send-back\n- \\- 2 findings &lt;/details&gt; \\*dropped\\* by the verifier \\# not a heading\n";
    assert!(md.contains(expected), "{md}");
    let notes = md.find("## Review notes").unwrap();
    let not_reported = md.find("## Not reported").unwrap();
    let findings = md.find("### 1. Hard-coded key").unwrap();
    assert!(findings < notes && notes < not_reported, "{md}");
    let prose = md.split("## Findings JSON").next().unwrap();
    assert!(!prose.contains("</details>"), "{md}");
}

/// Spec `repo-audit`, Too many notes refused: more than 10 notes, or a
/// note longer than 300 characters, fails with no `out/result.json`.
#[test]
fn too_many_or_too_long_notes_fail_with_no_result() {
    let dir = snapshot_worktree();
    let doc = |notes: serde_json::Value| {
        serde_json::json!({"version": 1, "findings": [], "notes": notes}).to_string()
    };
    let eleven: Vec<String> = (1..=11).map(|i| format!("note {i}")).collect();
    let err = fails_without_result(dir.path(), doc(serde_json::json!(eleven)).as_bytes());
    assert!(err.contains("notes") && err.contains("11"), "{err}");
    let long = "é".repeat(301);
    let err = fails_without_result(dir.path(), doc(serde_json::json!([long])).as_bytes());
    assert!(err.contains("notes") && err.contains("300"), "{err}");
    // The limits themselves pass: 10 notes of 300 characters.
    let ok: Vec<String> = (0..10).map(|_| "é".repeat(300)).collect();
    std::fs::write(
        dir.path().join("audit/candidates.json"),
        doc(serde_json::json!(ok)),
    )
    .unwrap();
    audit::finish(dir.path(), None, &[]).unwrap();
    assert!(dir.path().join("out/result.json").exists());
}
