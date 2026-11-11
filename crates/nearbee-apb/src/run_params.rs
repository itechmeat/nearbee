//! The run's string params for a Nearbee command started by a script node
//! (design 8.2): read from the run directory apb names in `APB_RUN_DIR`
//! (`run.yaml`, `params`), so no other crate knows apb's run file layout.

use std::collections::BTreeMap;

/// Reads `$APB_RUN_DIR/run.yaml` and returns its `params` as string
/// values (apb `RunConfig.params`, `crates/apb-engine/src/run_config.rs`).
/// A missing variable, a missing or unreadable file, or a document whose
/// `params` is not a string map is an error.
pub fn from_env() -> Result<BTreeMap<String, String>, String> {
    from_run_dir(
        std::env::var_os("APB_RUN_DIR")
            .as_deref()
            .map(std::path::Path::new),
    )
}

/// [`from_env`] for a given run directory value (`None`: the variable is
/// not set).
pub fn from_run_dir(dir: Option<&std::path::Path>) -> Result<BTreeMap<String, String>, String> {
    let dir = dir.ok_or_else(|| {
        "APB_RUN_DIR is not set: this command runs only inside an apb run".to_owned()
    })?;
    let path = dir.join("run.yaml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let root: yaml_serde::Value = yaml_serde::from_str(&text)
        .map_err(|e| format!("{} does not parse: {e}", path.display()))?;
    let Some(yaml_serde::Value::Mapping(params)) = root.get("params") else {
        return Err(format!("{}: params is not a map", path.display()));
    };
    let mut out = BTreeMap::new();
    for (k, v) in params {
        let (Some(key), Some(value)) = (k.as_str(), v.as_str()) else {
            return Err(format!(
                "{}: params carries a non string entry",
                path.display()
            ));
        };
        out.insert(key.to_owned(), value.to_owned());
    }
    Ok(out)
}
