//! The dated catalog snapshot of NEAR AI Cloud models.
//!
//! The snapshot is embedded verbatim from
//! `docs/reference/near-ai-cloud-models.json` (task 2.1, design 3). The
//! tier check reads ownership from it: an `attested` listing may name only
//! models the snapshot lists with owner `nearai`. A model given as
//! `nearai/<id>` is looked up with the prefix stripped.

use std::sync::OnceLock;

/// The verbatim catalog snapshot (see the `fetched` field inside it for
/// its date).
pub const CATALOG_JSON: &str = include_str!("../../../docs/reference/near-ai-cloud-models.json");

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ModelEntry {
    id: String,
    owned_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CatalogFile {
    models: Vec<ModelEntry>,
}

/// The parsed catalog snapshot.
#[derive(Debug, Clone)]
pub struct Catalog {
    models: Vec<ModelEntry>,
}

/// The embedded snapshot failed to parse or is missing a field. This can
/// only happen when the reference file itself is corrupt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("near-ai-cloud-models.json: {message}")]
pub struct CatalogError {
    message: String,
}

static CATALOG: OnceLock<Result<Catalog, CatalogError>> = OnceLock::new();

impl Catalog {
    /// Parse the embedded snapshot. The snapshot is checked in and known
    /// good, so this never fails in practice; the shared parse result is
    /// cached.
    pub fn embedded() -> Result<&'static Catalog, &'static CatalogError> {
        CATALOG
            .get_or_init(|| match serde_json::from_str::<CatalogFile>(CATALOG_JSON) {
                Ok(file) => Ok(Catalog {
                    models: file.models,
                }),
                Err(e) => Err(CatalogError {
                    message: e.to_string(),
                }),
            })
            .as_ref()
    }

    /// The owner the snapshot records for `id`, if it lists it. A model
    /// given as `nearai/<id>` is looked up with the prefix stripped
    /// (design 3).
    pub fn owned_by(&self, id: &str) -> Option<&str> {
        let stripped = id.strip_prefix("nearai/").unwrap_or(id);
        self.models
            .iter()
            .find(|m| m.id == stripped)
            .map(|m| m.owned_by.as_str())
    }

    /// True when the snapshot lists `id` with owner `nearai`.
    pub fn is_nearai(&self, id: &str) -> bool {
        self.owned_by(id) == Some("nearai")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded snapshot parses (task 2.1).
    #[test]
    fn embedded_catalog_parses() {
        let catalog = Catalog::embedded().expect("the checked-in catalog parses");
        assert!(!catalog.models.is_empty(), "the catalog is not empty");
        let fetched: serde_json::Value = serde_json::from_str(CATALOG_JSON).expect("json parses");
        let count = fetched["models"].as_array().expect("models array").len();
        assert_eq!(catalog.models.len(), count);
    }

    /// The two models the change names are in the snapshot and owned by
    /// `nearai` (task 2.1).
    #[test]
    fn deepseek_and_qwen_are_owned_by_nearai() {
        let catalog = Catalog::embedded().expect("the checked-in catalog parses");
        assert_eq!(
            catalog.owned_by("deepseek-ai/DeepSeek-V4.1-Flash-1017"),
            Some("nearai")
        );
        assert_eq!(catalog.owned_by("Qwen/Qwen3.8-27B"), Some("nearai"));
    }

    /// An `anthropic/` model is in the snapshot but is not owned by
    /// `nearai` (task 2.1).
    #[test]
    fn an_anthropic_model_is_not_nearai() {
        let catalog = Catalog::embedded().expect("the checked-in catalog parses");
        let owner = catalog
            .owned_by("anthropic/claude-fable-5")
            .expect("the snapshot lists the model");
        assert_ne!(owner, "nearai", "an anthropic model is not nearai");
        assert!(!catalog.is_nearai("anthropic/claude-fable-5"));
    }

    /// A `nearai/` prefix is stripped before the lookup, and an unknown
    /// model has no owner (design 3).
    #[test]
    fn nearai_prefix_is_stripped_and_unknown_ids_have_no_owner() {
        let catalog = Catalog::embedded().expect("the checked-in catalog parses");
        assert!(catalog.is_nearai("nearai/Qwen/Qwen3.8-27B"));
        assert_eq!(catalog.owned_by("nearai/some-new-model"), None);
        assert_eq!(catalog.owned_by("no-such-model"), None);
    }
}
