//! The fixture builder is test support: every workspace crate names it
//! only under `[dev-dependencies]`, so no shipped crate carries it.

use std::path::Path;

/// The dependency tables of a manifest that name `dep`, by table header.
fn tables_naming(manifest: &str, dep: &str) -> Vec<String> {
    let mut table = String::new();
    let mut out = Vec::new();
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            table = t.to_owned();
        } else if t.split(['=', '.', ' ']).next() == Some(dep) {
            out.push(table.clone());
        }
    }
    out
}

#[test]
fn every_crate_takes_the_fixture_builder_only_as_a_dev_dependency() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut users = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let manifest = entry.unwrap().path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        for table in tables_naming(&text, "nearbee-market-fixture") {
            assert_eq!(
                table,
                "[dev-dependencies]",
                "{} names the fixture builder in {table}",
                manifest.display()
            );
            users.push(manifest.clone());
        }
    }
    // The two crates whose tests mock the market do use it.
    assert_eq!(users.len(), 2, "{users:?}");
}
