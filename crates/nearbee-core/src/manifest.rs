//! Result manifest v1: the contract a market playbook's `out/result.json`
//! meets, and the validator that checks it before anything is delivered.
//!
//! Validation fails closed: a missing, unreadable or invalid manifest is an
//! error, never a partial result. Every file is read once with a bounded
//! read; its length and SHA-256 are compared with the manifest and the bytes
//! that were checked are returned, so what is delivered later is exactly what
//! was validated.
//!
//! Containment: `out` must be a real directory (not a symbolic link) inside
//! the worktree, and every entry must be a relative path of normal
//! components whose parents are real directories and whose final component
//! is a regular file. A process the run left behind may still change the
//! worktree while it is validated, so nothing is resolved by path twice:
//! `out` is opened once as a directory descriptor, and every entry is
//! resolved from it one component at a time with `openat` and
//! `O_NOFOLLOW` (parents also `O_DIRECTORY`, the leaf `O_NONBLOCK` so a FIFO
//! opens at once instead of blocking). A link swapped in anywhere on the
//! path fails the open; the opened leaf must be a regular file before a byte
//! is read. Validation needs a unix platform; elsewhere it fails closed.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Limits;

/// The manifest's file name inside `out/`.
pub const MANIFEST_FILE: &str = "result.json";
/// The output directory inside the worktree.
pub const OUT_DIR: &str = "out";
/// Largest manifest file read.
pub const MANIFEST_MAX_BYTES: u64 = 1024 * 1024;

/// A manifest entry: a file under `out/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// Who may see an evidence entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Private,
}

/// An evidence entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub kind: String,
    pub visibility: Visibility,
}

/// Manifest version 1 as written by a playbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub markdown: Entry,
    #[serde(default)]
    pub files: Vec<Entry>,
    #[serde(default)]
    pub evidence: Vec<EvidenceEntry>,
}

/// A file whose bytes were checked against the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedFile {
    pub path: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// A checked evidence file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEvidence {
    pub file: ValidatedFile,
    pub kind: String,
    pub visibility: Visibility,
}

/// The result of a successful validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedManifest {
    pub markdown: ValidatedFile,
    /// The Markdown entry as text (it was checked to be UTF-8).
    pub markdown_text: String,
    pub files: Vec<ValidatedFile>,
    pub evidence: Vec<ValidatedEvidence>,
}

/// A validation failure. Nothing is delivered after any of these.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the result manifest out/result.json is missing")]
    Missing,
    #[error("the result manifest cannot be read: {0}")]
    Unreadable(String),
    #[error("the result manifest is invalid: {0}")]
    Invalid(String),
    #[error("unsupported manifest version {0}; this Nearbee reads version 1")]
    UnsupportedVersion(u64),
    #[error("duplicate path in the manifest: {0}")]
    DuplicatePath(String),
    #[error("containment: {path}: {reason}")]
    Containment { path: String, reason: String },
    #[error("{path}: size {actual} does not match the manifest size {expected}")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("{path}: SHA-256 does not match the manifest")]
    HashMismatch { path: String },
    #[error("{path}: larger than the limit of {limit} bytes")]
    TooLarge { path: String, limit: u64 },
    #[error("too many files: {count}, the limit is {limit}")]
    TooManyFiles { count: usize, limit: usize },
    #[error("total size {total} is over the limit of {limit} bytes")]
    TotalTooLarge { total: u64, limit: u64 },
    #[error("{path}: the Markdown entry is not valid UTF-8")]
    NotUtf8 { path: String },
    #[error("the Markdown text is {size} bytes, over the limit of {limit}")]
    MarkdownTooLarge { size: u64, limit: u64 },
}

/// Parses manifest JSON: version first (so a newer version gets its own
/// error), then the strict v1 schema.
pub fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| ManifestError::Invalid(e.to_string()))?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(1) => {}
        Some(v) => return Err(ManifestError::UnsupportedVersion(v)),
        None => return Err(ManifestError::Invalid("`version` is missing".into())),
    }
    let m: Manifest =
        serde_json::from_value(value).map_err(|e| ManifestError::Invalid(e.to_string()))?;
    for (path, sha) in m.entries() {
        if sha.len() != 64 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(ManifestError::Invalid(format!(
                "{path}: sha256 must be 64 lowercase hex digits"
            )));
        }
    }
    let mut seen = BTreeSet::new();
    for (path, _) in m.entries() {
        if !seen.insert(normalized_key(path)) {
            return Err(ManifestError::DuplicatePath(path.to_owned()));
        }
    }
    Ok(m)
}

impl Manifest {
    fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        std::iter::once((self.markdown.path.as_str(), self.markdown.sha256.as_str()))
            .chain(
                self.files
                    .iter()
                    .map(|e| (e.path.as_str(), e.sha256.as_str())),
            )
            .chain(
                self.evidence
                    .iter()
                    .map(|e| (e.path.as_str(), e.sha256.as_str())),
            )
    }
}

/// Two spellings of one path (`a//b`, `./a/b`) count as the same path.
fn normalized_key(path: &str) -> String {
    Path::new(path)
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Validates `<worktree>/out/result.json` and every file it names.
pub fn validate(worktree: &Path, limits: &Limits) -> Result<ValidatedManifest, ManifestError> {
    let out = OutDir::open(worktree)?;
    let manifest_bytes = match open_entry(&out, MANIFEST_FILE) {
        Ok(file) => read_bounded(file, MANIFEST_MAX_BYTES, MANIFEST_FILE).map_err(|e| match e {
            ManifestError::TooLarge { limit, .. } => {
                ManifestError::Unreadable(format!("larger than {limit} bytes"))
            }
            other => other,
        })?,
        Err(WalkError::NotFound) => return Err(ManifestError::Missing),
        Err(WalkError::Containment(reason)) => {
            return Err(ManifestError::Containment {
                path: MANIFEST_FILE.into(),
                reason,
            });
        }
        Err(WalkError::Io(e)) => return Err(ManifestError::Unreadable(e)),
    };
    let m = parse(&manifest_bytes)?;

    let count = 1 + m.files.len() + m.evidence.len();
    if count > limits.files {
        return Err(ManifestError::TooManyFiles {
            count,
            limit: limits.files,
        });
    }

    let mut total = 0u64;
    let md_limit = limits.markdown_bytes.min(limits.file_bytes);
    let markdown = check_entry(&out, &m.markdown, md_limit, limits, &mut total)?;
    let markdown_text =
        String::from_utf8(markdown.bytes.clone()).map_err(|_| ManifestError::NotUtf8 {
            path: markdown.path.clone(),
        })?;
    let mut files = Vec::with_capacity(m.files.len());
    for e in &m.files {
        files.push(check_entry(&out, e, limits.file_bytes, limits, &mut total)?);
    }
    let mut evidence = Vec::with_capacity(m.evidence.len());
    for e in &m.evidence {
        let entry = Entry {
            path: e.path.clone(),
            size: e.size,
            sha256: e.sha256.clone(),
        };
        let file = check_entry(&out, &entry, limits.file_bytes, limits, &mut total)?;
        evidence.push(ValidatedEvidence {
            file,
            kind: e.kind.clone(),
            visibility: e.visibility,
        });
    }
    Ok(ValidatedManifest {
        markdown,
        markdown_text,
        files,
        evidence,
    })
}

/// Checks the final deliverable text (Markdown plus appended blocks) against
/// the Markdown limit.
pub fn check_final_markdown(text: &str, limits: &Limits) -> Result<(), ManifestError> {
    let size = text.len() as u64;
    if size > limits.markdown_bytes {
        return Err(ManifestError::MarkdownTooLarge {
            size,
            limit: limits.markdown_bytes,
        });
    }
    Ok(())
}

/// Reads at most `limit + 1` bytes; more than `limit` is an error. The
/// reader is never asked for more than `limit + 1` bytes.
fn read_bounded<R: Read>(reader: R, limit: u64, path: &str) -> Result<Vec<u8>, ManifestError> {
    let mut buf = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| ManifestError::Unreadable(format!("{path}: {e}")))?;
    if buf.len() as u64 > limit {
        return Err(ManifestError::TooLarge {
            path: path.to_owned(),
            limit,
        });
    }
    Ok(buf)
}

enum WalkError {
    NotFound,
    Containment(String),
    Io(String),
}

/// The plain components of a manifest path, or why it is refused.
fn components(rel: &str) -> Result<Vec<&std::ffi::OsStr>, String> {
    let p = Path::new(rel);
    if rel.is_empty() {
        return Err("empty path".into());
    }
    if p.is_absolute() {
        return Err("absolute path".into());
    }
    let mut names = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(name) => names.push(name),
            _ => {
                return Err("only plain path components are allowed".into());
            }
        }
    }
    if names.is_empty() {
        return Err("names no file".into());
    }
    Ok(names)
}

/// `<worktree>/out`, opened once as a directory descriptor; every entry is
/// resolved relative to it.
#[cfg(unix)]
struct OutDir(std::os::fd::OwnedFd);

#[cfg(unix)]
impl OutDir {
    fn open(worktree: &Path) -> Result<Self, ManifestError> {
        use rustix::fs::{CWD, Mode, OFlags, openat};
        use rustix::io::Errno;
        let root = worktree
            .canonicalize()
            .map_err(|e| ManifestError::Unreadable(format!("worktree: {e}")))?;
        let root = openat(
            CWD,
            &root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| ManifestError::Unreadable(format!("worktree: {e}")))?;
        match openat(&root, OUT_DIR, DIR_FLAGS, Mode::empty()) {
            Ok(fd) => Ok(Self(fd)),
            Err(Errno::NOENT) => Err(ManifestError::Missing),
            Err(Errno::LOOP | Errno::NOTDIR) => Err(ManifestError::Containment {
                path: OUT_DIR.into(),
                reason: if is_symlink(&root, OUT_DIR.as_ref()) {
                    "is a symbolic link".into()
                } else {
                    "is not a directory".into()
                },
            }),
            Err(e) => Err(ManifestError::Unreadable(format!("out: {e}"))),
        }
    }
}

#[cfg(unix)]
const DIR_FLAGS: rustix::fs::OFlags = rustix::fs::OFlags::RDONLY
    .union(rustix::fs::OFlags::DIRECTORY)
    .union(rustix::fs::OFlags::NOFOLLOW)
    .union(rustix::fs::OFlags::CLOEXEC);

/// Whether `name` in `dir` is itself a symbolic link (used only to word an
/// error; the refusal does not depend on it).
#[cfg(unix)]
fn is_symlink(dir: impl std::os::fd::AsFd, name: &std::ffi::OsStr) -> bool {
    rustix::fs::statat(dir, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
        .map(|st| rustix::fs::FileType::from_raw_mode(st.st_mode) == rustix::fs::FileType::Symlink)
        .unwrap_or(false)
}

/// Opens `rel` under `out` through directory descriptors, one component at
/// a time, never following a symbolic link and never resolving a path from
/// the top again: a parent replaced by a link at any moment fails its own
/// `openat`. The returned file is a regular file.
#[cfg(unix)]
fn open_entry(out: &OutDir, rel: &str) -> Result<File, WalkError> {
    use rustix::fs::{Mode, OFlags, openat};
    use rustix::io::Errno;
    use std::os::fd::AsFd;
    let names = components(rel).map_err(WalkError::Containment)?;
    let (leaf, parents) = names.split_last().expect("components is never empty");
    let mut dir: Option<std::os::fd::OwnedFd> = None;
    for name in parents {
        let base = dir.as_ref().map_or(out.0.as_fd(), |d| d.as_fd());
        match openat(base, *name, DIR_FLAGS, Mode::empty()) {
            Ok(fd) => dir = Some(fd),
            Err(Errno::NOENT) => return Err(WalkError::NotFound),
            Err(Errno::LOOP | Errno::NOTDIR) => {
                return Err(WalkError::Containment(if is_symlink(base, name) {
                    "is or passes a symbolic link".into()
                } else {
                    "a parent component is not a directory".into()
                }));
            }
            Err(e) => return Err(WalkError::Io(e.to_string())),
        }
    }
    let base = dir.as_ref().map_or(out.0.as_fd(), |d| d.as_fd());
    let flags =
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC;
    let file = match openat(base, *leaf, flags, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(Errno::NOENT) => return Err(WalkError::NotFound),
        Err(Errno::LOOP) => {
            return Err(WalkError::Containment(
                "is or passes a symbolic link".into(),
            ));
        }
        Err(e) => return Err(WalkError::Io(e.to_string())),
    };
    let meta = file.metadata().map_err(|e| WalkError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(WalkError::Containment("is not a regular file".into()));
    }
    Ok(file)
}

/// Off unix there is no descriptor-relative open in std, so validation
/// fails closed instead of resolving paths that can change under it.
#[cfg(not(unix))]
struct OutDir;

#[cfg(not(unix))]
impl OutDir {
    fn open(_worktree: &Path) -> Result<Self, ManifestError> {
        Err(ManifestError::Unreadable(
            "result validation needs a unix platform".into(),
        ))
    }
}

#[cfg(not(unix))]
fn open_entry(_out: &OutDir, _rel: &str) -> Result<File, WalkError> {
    Err(WalkError::Io(
        "result validation needs a unix platform".into(),
    ))
}

/// Opens and reads one entry, mapping resolution errors to manifest errors
/// that name the entry.
fn read_entry(out: &OutDir, rel: &str, limit: u64) -> Result<Vec<u8>, ManifestError> {
    let file = open_entry(out, rel).map_err(|w| match w {
        WalkError::NotFound => ManifestError::Containment {
            path: rel.into(),
            reason: "does not exist".into(),
        },
        WalkError::Containment(reason) => ManifestError::Containment {
            path: rel.into(),
            reason,
        },
        WalkError::Io(m) => ManifestError::Unreadable(format!("{rel}: {m}")),
    })?;
    read_bounded(file, limit, rel)
}

fn check_entry(
    out: &OutDir,
    e: &Entry,
    limit: u64,
    limits: &Limits,
    total: &mut u64,
) -> Result<ValidatedFile, ManifestError> {
    // Path syntax first, so a traversal is refused without touching disk.
    components(&e.path).map_err(|reason| ManifestError::Containment {
        path: e.path.clone(),
        reason,
    })?;
    if e.size > limit {
        return Err(ManifestError::TooLarge {
            path: e.path.clone(),
            limit,
        });
    }
    let bytes = read_entry(out, &e.path, limit)?;
    let actual = bytes.len() as u64;
    if actual != e.size {
        return Err(ManifestError::SizeMismatch {
            path: e.path.clone(),
            expected: e.size,
            actual,
        });
    }
    let sha = hex::encode(Sha256::digest(&bytes));
    if sha != e.sha256 {
        return Err(ManifestError::HashMismatch {
            path: e.path.clone(),
        });
    }
    *total += actual;
    if *total > limits.total_bytes {
        return Err(ManifestError::TotalTooLarge {
            total: *total,
            limit: limits.total_bytes,
        });
    }
    Ok(ValidatedFile {
        path: e.path.clone(),
        sha256: sha,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::sha256_hex;
    use serde_json::json;
    use std::io::Cursor;
    use std::path::PathBuf;

    struct Tree {
        dir: tempfile::TempDir,
    }

    impl Tree {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("out")).unwrap();
            Self { dir }
        }
        fn root(&self) -> &Path {
            self.dir.path()
        }
        fn out(&self) -> PathBuf {
            self.root().join("out")
        }
        fn file(&self, rel: &str, bytes: &[u8]) -> serde_json::Value {
            let p = self.out().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, bytes).unwrap();
            entry(rel, bytes)
        }
        fn manifest(&self, v: serde_json::Value) {
            std::fs::write(self.out().join(MANIFEST_FILE), v.to_string()).unwrap();
        }
        fn validate(&self) -> Result<ValidatedManifest, ManifestError> {
            validate(self.root(), &Limits::default())
        }
    }

    fn entry(rel: &str, bytes: &[u8]) -> serde_json::Value {
        json!({"path": rel, "size": bytes.len(), "sha256": sha256_hex(bytes)})
    }

    fn containment(r: Result<ValidatedManifest, ManifestError>) -> String {
        match r {
            Err(ManifestError::Containment { path, .. }) => path,
            other => panic!("expected a containment error, got {other:?}"),
        }
    }

    #[test]
    fn minimal_valid_manifest() {
        let t = Tree::new();
        let md = t.file("deliverable.md", b"# Note\n\nHello.\n");
        t.manifest(json!({"version": 1, "markdown": md}));
        let v = t.validate().unwrap();
        assert_eq!(v.markdown.path, "deliverable.md");
        assert_eq!(v.markdown_text, "# Note\n\nHello.\n");
        assert_eq!(v.markdown.bytes, b"# Note\n\nHello.\n");
        assert!(v.files.is_empty() && v.evidence.is_empty());
    }

    #[test]
    fn files_and_evidence_are_validated() {
        let t = Tree::new();
        let md = t.file("deliverable.md", b"x");
        let f = t.file("data/table.csv", b"a,b\n1,2\n");
        let mut ev = t.file("ev/log.txt", b"log");
        ev["kind"] = json!("log");
        ev["visibility"] = json!("private");
        t.manifest(json!({"version": 1, "markdown": md, "files": [f], "evidence": [ev]}));
        let v = t.validate().unwrap();
        assert_eq!(v.files[0].bytes, b"a,b\n1,2\n");
        assert_eq!(v.evidence[0].visibility, Visibility::Private);
        assert_eq!(v.evidence[0].kind, "log");
    }

    #[test]
    fn unknown_version_is_refused() {
        let t = Tree::new();
        let md = t.file("d.md", b"x");
        t.manifest(json!({"version": 2, "markdown": md}));
        assert_eq!(t.validate(), Err(ManifestError::UnsupportedVersion(2)));
    }

    #[test]
    fn unknown_field_is_refused_naming_it() {
        let t = Tree::new();
        let mut md = t.file("d.md", b"x");
        md["mime"] = json!("text/markdown");
        t.manifest(json!({"version": 1, "markdown": md}));
        match t.validate() {
            Err(ManifestError::Invalid(m)) => assert!(m.contains("mime"), "{m}"),
            other => panic!("{other:?}"),
        }
        let md = t.file("d.md", b"x");
        t.manifest(json!({"version": 1, "markdown": md, "summary": "hi"}));
        match t.validate() {
            Err(ManifestError::Invalid(m)) => assert!(m.contains("summary"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn duplicate_path_is_refused() {
        let t = Tree::new();
        let md = t.file("d.md", b"x");
        let mut dup = md.clone();
        dup["path"] = json!("./d.md");
        t.manifest(json!({"version": 1, "markdown": md, "files": [dup]}));
        assert_eq!(
            t.validate(),
            Err(ManifestError::DuplicatePath("./d.md".into()))
        );
    }

    #[test]
    fn markdown_in_a_subdirectory_is_accepted() {
        let t = Tree::new();
        let md = t.file("docs/deliverable.md", b"# A\n");
        t.manifest(json!({"version": 1, "markdown": md}));
        assert_eq!(t.validate().unwrap().markdown.path, "docs/deliverable.md");
    }

    #[test]
    fn bad_sha_format_is_refused() {
        let t = Tree::new();
        t.file("d.md", b"x");
        t.manifest(json!({"version": 1, "markdown": {"path": "d.md", "size": 1, "sha256": "ABC"}}));
        assert!(matches!(t.validate(), Err(ManifestError::Invalid(_))));
    }

    #[test]
    fn traversal_is_refused_and_nothing_outside_is_read() {
        let t = Tree::new();
        std::fs::write(t.root().join("secrets.txt"), b"s").unwrap();
        t.manifest(json!({"version": 1, "markdown": entry("../secrets.txt", b"s")}));
        assert_eq!(containment(t.validate()), "../secrets.txt");
        t.manifest(json!({"version": 1, "markdown": entry("a/../../secrets.txt", b"s")}));
        assert_eq!(containment(t.validate()), "a/../../secrets.txt");
    }

    #[test]
    fn absolute_path_is_refused() {
        let t = Tree::new();
        let abs = t.root().join("secrets.txt");
        std::fs::write(&abs, b"s").unwrap();
        let p = abs.to_string_lossy().into_owned();
        t.manifest(json!({"version": 1, "markdown": entry(&p, b"s")}));
        assert_eq!(containment(t.validate()), p);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_out_is_refused() {
        let outside = tempfile::tempdir().unwrap();
        let md = b"x";
        std::fs::write(outside.path().join("d.md"), md).unwrap();
        std::fs::write(
            outside.path().join(MANIFEST_FILE),
            json!({"version": 1, "markdown": entry("d.md", md)}).to_string(),
        )
        .unwrap();
        let wt = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), wt.path().join("out")).unwrap();
        assert_eq!(
            containment(validate(wt.path(), &Limits::default())),
            OUT_DIR
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_entry_is_refused_even_inside_out() {
        let t = Tree::new();
        let md = t.file("real.md", b"x");
        std::os::unix::fs::symlink(t.out().join("real.md"), t.out().join("report.md")).unwrap();
        let mut e = md.clone();
        e["path"] = json!("report.md");
        t.manifest(json!({"version": 1, "markdown": e}));
        assert_eq!(containment(t.validate()), "report.md");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_directory_is_refused() {
        let t = Tree::new();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("d.md"), b"x").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), t.out().join("sub")).unwrap();
        t.manifest(json!({"version": 1, "markdown": entry("sub/d.md", b"x")}));
        assert_eq!(containment(t.validate()), "sub/d.md");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_manifest_is_refused() {
        let t = Tree::new();
        let md = t.file("d.md", b"x");
        let elsewhere = tempfile::tempdir().unwrap();
        let m = elsewhere.path().join("m.json");
        std::fs::write(&m, json!({"version": 1, "markdown": md}).to_string()).unwrap();
        std::os::unix::fs::symlink(&m, t.out().join(MANIFEST_FILE)).unwrap();
        assert_eq!(containment(t.validate()), MANIFEST_FILE);
    }

    #[cfg(unix)]
    #[test]
    fn non_regular_file_is_refused() {
        let t = Tree::new();
        let fifo = t.out().join("pipe.md");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        t.manifest(json!({"version": 1, "markdown": entry("pipe.md", b"x")}));
        assert_eq!(containment(t.validate()), "pipe.md");
        // A directory named as a file is refused too.
        std::fs::create_dir(t.out().join("dir.md")).unwrap();
        t.manifest(json!({"version": 1, "markdown": entry("dir.md", b"x")}));
        assert_eq!(containment(t.validate()), "dir.md");
    }

    /// Runs `read_entry` on another thread; `None` when it did not return
    /// within five seconds (it hangs).
    #[cfg(unix)]
    fn read_with_deadline(out: OutDir, rel: &str) -> Option<Result<Vec<u8>, ManifestError>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let rel = rel.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(read_entry(&out, &rel, 1024));
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
    }

    #[cfg(unix)]
    #[test]
    fn fifo_swapped_in_during_validation_is_refused_without_hanging() {
        let t = Tree::new();
        t.file("d.md", b"real");
        let out = OutDir::open(t.root()).expect("out opens");
        // A process the run left behind replaces the file with a FIFO that
        // nobody writes to, while validation runs.
        std::fs::remove_file(t.out().join("d.md")).unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(t.out().join("d.md"))
            .status()
            .unwrap();
        assert!(status.success());
        let got = read_with_deadline(out, "d.md").expect("the open hung on a FIFO");
        assert!(
            matches!(got, Err(ManifestError::Containment { ref path, .. }) if path == "d.md"),
            "{got:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_swapped_in_during_validation_is_not_followed() {
        let t = Tree::new();
        t.file("d.md", b"real");
        let out = OutDir::open(t.root()).expect("out opens");
        // The same file under a link: following it would pass the inode check.
        std::fs::rename(t.out().join("d.md"), t.out().join("real.md")).unwrap();
        std::os::unix::fs::symlink(t.out().join("real.md"), t.out().join("d.md")).unwrap();
        let got = read_with_deadline(out, "d.md").expect("the open hung");
        assert!(
            matches!(got, Err(ManifestError::Containment { ref path, .. }) if path == "d.md"),
            "{got:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn parent_directory_swapped_for_a_symlink_during_validation_is_refused() {
        let t = Tree::new();
        t.file("reports/d.md", b"real");
        let out = OutDir::open(t.root()).expect("out opens");
        // While validation runs (`out` already open), a process the run left
        // behind moves `reports` out of `out/` and puts a link to it in its
        // place: the leaf is the same file, only a parent is now a link.
        let elsewhere = tempfile::tempdir().unwrap();
        let moved = elsewhere.path().join("reports");
        std::fs::rename(t.out().join("reports"), &moved).unwrap();
        std::os::unix::fs::symlink(&moved, t.out().join("reports")).unwrap();
        let got = read_with_deadline(out, "reports/d.md").expect("the open hung");
        assert!(
            matches!(got, Err(ManifestError::Containment { ref path, .. }) if path == "reports/d.md"),
            "{got:?}"
        );
    }

    #[test]
    fn hash_mismatch_names_the_path() {
        let t = Tree::new();
        t.file("d.md", b"real");
        t.manifest(json!({"version": 1, "markdown": entry("d.md", b"fake")}));
        assert_eq!(
            t.validate(),
            Err(ManifestError::HashMismatch {
                path: "d.md".into()
            })
        );
    }

    #[test]
    fn size_mismatch_names_the_path() {
        let t = Tree::new();
        t.file("d.md", b"real");
        let mut e = entry("d.md", b"real");
        e["size"] = json!(3);
        t.manifest(json!({"version": 1, "markdown": e}));
        assert!(matches!(
            t.validate(),
            Err(ManifestError::SizeMismatch { ref path, expected: 3, actual: 4 }) if path == "d.md"
        ));
    }

    /// A reader that records how many bytes were pulled from it.
    struct Counting<R> {
        inner: R,
        pulled: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.pulled.set(self.pulled.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn bounded_read_stops_at_limit_plus_one() {
        let pulled = std::rc::Rc::new(std::cell::Cell::new(0));
        let r = Counting {
            inner: Cursor::new(vec![7u8; 10_000]),
            pulled: pulled.clone(),
        };
        let err = read_bounded(r, 100, "big.bin").unwrap_err();
        assert_eq!(
            err,
            ManifestError::TooLarge {
                path: "big.bin".into(),
                limit: 100
            }
        );
        assert_eq!(pulled.get(), 101);
        assert_eq!(
            read_bounded(Cursor::new(vec![1u8; 100]), 100, "ok")
                .unwrap()
                .len(),
            100
        );
    }

    #[test]
    fn file_over_the_limit_is_refused() {
        let t = Tree::new();
        let md = t.file("d.md", b"x");
        let big = vec![b'a'; 2048];
        // The manifest lies about the size; the bounded read still stops.
        std::fs::write(t.out().join("big.bin"), &big).unwrap();
        let mut e = entry("big.bin", &big[..1000]);
        e["size"] = json!(1000);
        t.manifest(json!({"version": 1, "markdown": md, "files": [e]}));
        let limits = Limits {
            file_bytes: 1024,
            ..Limits::default()
        };
        assert_eq!(
            validate(t.root(), &limits),
            Err(ManifestError::TooLarge {
                path: "big.bin".into(),
                limit: 1024
            })
        );
        // An honest manifest size over the limit is refused before reading.
        t.manifest(json!({"version": 1, "markdown": md, "files": [entry("big.bin", &big)]}));
        assert!(matches!(
            validate(t.root(), &limits),
            Err(ManifestError::TooLarge { .. })
        ));
    }

    #[test]
    fn too_many_files_is_refused() {
        let t = Tree::new();
        let md = t.file("d.md", b"x");
        let files: Vec<_> = (0..3).map(|i| t.file(&format!("f{i}"), b"y")).collect();
        t.manifest(json!({"version": 1, "markdown": md, "files": files}));
        let limits = Limits {
            files: 3,
            ..Limits::default()
        };
        assert_eq!(
            validate(t.root(), &limits),
            Err(ManifestError::TooManyFiles { count: 4, limit: 3 })
        );
    }

    #[test]
    fn total_over_the_limit_is_refused() {
        let t = Tree::new();
        let md = t.file("d.md", b"x");
        let a = t.file("a", &[1u8; 600]);
        let b = t.file("b", &[2u8; 600]);
        t.manifest(json!({"version": 1, "markdown": md, "files": [a, b]}));
        let limits = Limits {
            total_bytes: 1000,
            ..Limits::default()
        };
        assert!(matches!(
            validate(t.root(), &limits),
            Err(ManifestError::TotalTooLarge { limit: 1000, .. })
        ));
    }

    #[test]
    fn markdown_limit_boundaries() {
        let t = Tree::new();
        let ok = vec![b'a'; 262_144];
        let md = t.file("d.md", &ok);
        t.manifest(json!({"version": 1, "markdown": md}));
        assert_eq!(t.validate().unwrap().markdown_text.len(), 262_144);

        let over = vec![b'a'; 262_145];
        let md = t.file("d.md", &over);
        t.manifest(json!({"version": 1, "markdown": md}));
        assert!(matches!(
            t.validate(),
            Err(ManifestError::TooLarge { limit: 262_144, .. })
        ));
    }

    #[test]
    fn markdown_must_be_utf8() {
        let t = Tree::new();
        let md = t.file("d.md", &[0xff, 0xfe, b'a']);
        t.manifest(json!({"version": 1, "markdown": md}));
        assert_eq!(
            t.validate(),
            Err(ManifestError::NotUtf8 {
                path: "d.md".into()
            })
        );
    }

    #[test]
    fn footer_that_pushes_over_the_limit_fails_the_final_check() {
        let limits = Limits::default();
        let body = "a".repeat(262_000);
        assert!(check_final_markdown(&body, &limits).is_ok());
        let full = format!("{body}{}", "f".repeat(500));
        assert_eq!(
            check_final_markdown(&full, &limits),
            Err(ManifestError::MarkdownTooLarge {
                size: 262_500,
                limit: 262_144
            })
        );
        assert!(check_final_markdown(&"a".repeat(262_144), &limits).is_ok());
    }

    #[test]
    fn missing_manifest_is_an_error() {
        let t = Tree::new();
        t.file("deliverable.md", b"x");
        assert_eq!(t.validate(), Err(ManifestError::Missing));
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            validate(empty.path(), &Limits::default()),
            Err(ManifestError::Missing)
        );
    }

    #[test]
    fn missing_entry_file_is_an_error() {
        let t = Tree::new();
        t.manifest(json!({"version": 1, "markdown": entry("gone.md", b"x")}));
        assert_eq!(containment(t.validate()), "gone.md");
    }
}
