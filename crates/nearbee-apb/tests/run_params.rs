//! `run_params::from_env` (spec `apb-runner`, Run parameters for a script
//! node): the fixture `run.yaml` replayed, the missing file and the
//! missing variable.

use std::path::Path;

#[test]
fn from_env_reads_the_run_params() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/apb");
    let params = nearbee_apb::run_params::from_run_dir(Some(&fixtures)).unwrap();
    assert_eq!(
        params.get("brief").map(String::as_str),
        Some("Audit the repository nearbee/itechmeat on GitHub.")
    );
    assert_eq!(params.get("feedback").map(String::as_str), Some(""));
    assert_eq!(params.get("inputs").map(String::as_str), Some("[]"));

    // No run directory variable: the command does not run inside apb.
    let err = nearbee_apb::run_params::from_run_dir(None).unwrap_err();
    assert!(err.contains("APB_RUN_DIR is not set"), "{err}");

    // The variable names a directory without run.yaml.
    let empty = tempfile::tempdir().unwrap();
    let err = nearbee_apb::run_params::from_run_dir(Some(empty.path())).unwrap_err();
    assert!(err.contains("cannot read"), "{err}");
}
