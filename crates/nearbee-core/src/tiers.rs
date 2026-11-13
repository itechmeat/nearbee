//! The tier check: what a listing's tier promises, what the execution
//! facts must satisfy, and which models the attestation evidence covers.
//!
//! Pure data and a pure `check` (design 2, 3). The facts come from the
//! `Runner` seam (`inspect`, added in group 4); the check runs against
//! the embedded catalog (`crate::catalog`).

use crate::catalog::Catalog;
use crate::listing::Tier;
use serde::Serialize;

/// A fact apb could not provide, with the reason (design 2: a fact is
/// unknown, never guessed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Known<T> {
    Yes(T),
    Unknown(String),
}

/// A profile reference as apb records it: name and scope
/// (`project`, `global` or `auto`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ProfileRef {
    pub name: String,
    pub scope: String,
}

/// One executor binding: the agent and the model it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutorFact {
    pub agent: String,
    pub model: String,
}

/// Everything the check and the statement need about one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileFact {
    /// The scope the profile was actually read from, `project` or
    /// `global`, even when the playbook's reference says `auto`; the
    /// outer key in `ExecutionFacts::profiles` keeps the reference as the
    /// playbook names it.
    pub reference: ProfileRef,
    pub executor: ExecutorFact,
    /// Declaration order kept: it decides routing.
    pub fallbacks: Vec<ExecutorFact>,
    /// Declaration order kept: it decides routing.
    pub tiers: Vec<(String, ExecutorFact)>,
    pub environment: String,
    /// Sorted.
    pub skills: Vec<String>,
    pub env_policy: String,
    /// Sorted.
    pub env_allow: Vec<String>,
}

/// What one node of the playbook is and uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeFact {
    pub id: String,
    pub kind: String,
    pub interactive: bool,
    pub profile: Option<ProfileRef>,
    pub connectors: Vec<String>,
    pub playbook_ref: Option<String>,
}

/// One model provider route from the workspace's `opencode.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderRoute {
    pub name: String,
    pub base_url: String,
    pub api_key_ref: String,
    /// The provider's model entries that alias or override a model (any
    /// key beyond display and metadata), each as `<id>: <keys>`, sorted.
    /// Left out of the serialized facts when empty, so a fingerprint taken
    /// before the field existed still matches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub model_overrides: Vec<String>,
}

/// The container the runner's instance runs in, read from the service
/// unit's command line (change `statement-sandbox`, spec `privacy-tiers`,
/// Sandbox fact).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SandboxFact {
    /// The hosts outbound connections are limited to; sorted, deduplicated.
    pub allowed_hosts: Vec<String>,
}

/// The facts the check and the statement are built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionFacts {
    pub playbook_id: String,
    pub default_profile: Option<ProfileRef>,
    /// Sorted by id.
    pub nodes: Vec<NodeFact>,
    /// Sorted by (name, scope).
    pub profiles: Vec<(ProfileRef, Known<ProfileFact>)>,
    /// Sorted by name.
    pub routes: Known<Vec<ProviderRoute>>,
    pub decisions_enabled: Known<bool>,
    pub compaction_enabled: Known<bool>,
    /// The listing's direct models, in order. Never filled by the runner:
    /// the caller copies them from the listing or its record with
    /// [`ExecutionFacts::with_direct_models`]. Left out of the serialized
    /// facts when empty, so an S4 fingerprint is unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_models: Vec<String>,
    /// The listing's signed models, in order; same rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signed_models: Vec<String>,
    /// The sandbox the instance runs in, when the runner found one. Left
    /// out of the serialized facts when absent, so the fingerprint of an
    /// instance without a sandbox is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxFact>,
}

impl ExecutionFacts {
    /// The facts with the listing's direct and signed models, which the
    /// runner never reports (design 5.1): used at `nearbee list` and in
    /// the drift pass, before the check, the fingerprint, the statement
    /// and the evidence set.
    pub fn with_direct_models(mut self, direct: &[String], signed: &[String]) -> Self {
        self.direct_models = direct.to_vec();
        self.signed_models = signed.to_vec();
        self
    }
}

/// The node kinds the check knows (design 3); any other kind is a breach.
const KNOWN_NODE_KINDS: [&str; 5] = ["start", "finish", "agent_task", "script", "prompt"];

/// The only process environment variable an attested profile's allowlist
/// may name (spec `privacy-tiers`, Attested side channels).
const ATTESTED_ENV_ALLOWED: [&str; 1] = ["NEARAI_API_KEY"];

/// The node kinds that run an agent: an attested listing has none of them
/// (S6 design 4.1).
const AGENT_NODE_KINDS: [&str; 2] = ["agent_task", "prompt"];

/// Why an attested listing may not run `opencode` (S6 design 4.1).
const OPENCODE_BREACH: &str = "the opencode agent runs in a shared background service with the starting user's configuration (apb starts it without --standalone); attested listings use Nearbee's direct calls only";

/// The exact base URL the `nearai` provider must point at (spec
/// `privacy-tiers`, Attested model route).
pub const NEARAI_BASE_URL: &str = "https://cloud-api.near.ai/v1";

/// The exact key reference the `nearai` provider must use (spec
/// `privacy-tiers`, Attested model route: the key is read from
/// `NEARAI_API_KEY`).
pub const NEARAI_API_KEY_REF: &str = "{env:NEARAI_API_KEY}";

/// Strip the `nearai/` prefix from a model id.
fn strip_nearai(model: &str) -> &str {
    model.strip_prefix("nearai/").unwrap_or(model)
}

/// True when the model is a catalog `nearai` model (spec `privacy-tiers`,
/// Attested model route); the agent is checked apart.
fn model_ok(catalog: &Catalog, model: &str) -> bool {
    model.starts_with("nearai/") && catalog.is_nearai(model)
}

/// One usable profile is one referenced by the default profile or by any
/// node, resolved by its exact (name, scope) reference.
pub(crate) fn usable_references(facts: &ExecutionFacts) -> Vec<ProfileRef> {
    let mut refs: Vec<ProfileRef> = facts.default_profile.clone().into_iter().collect();
    for node in &facts.nodes {
        if let Some(profile) = &node.profile {
            refs.push(profile.clone());
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn profile_fact<'a>(facts: &'a ExecutionFacts, reference: &ProfileRef) -> Option<&'a ProfileFact> {
    facts
        .profiles
        .iter()
        .find(|(r, f)| r == reference && matches!(f, Known::Yes(_)))
        .and_then(|(_, f)| match f {
            Known::Yes(fact) => Some(fact),
            Known::Unknown(_) => None,
        })
}

/// `Ok(())` when the facts satisfy the tier, otherwise every breach as one
/// English line naming the node, profile or model (design 3). The
/// structural refusals, an unknown decisions or compaction setting
/// included, apply to every tier; the model route, side channel and
/// decisions or compaction on refusals apply to `attested` only.
pub fn check(tier: Tier, facts: &ExecutionFacts, catalog: &Catalog) -> Result<(), Vec<String>> {
    let mut breaches: Vec<String> = Vec::new();
    for node in &facts.nodes {
        if !KNOWN_NODE_KINDS.contains(&node.kind.as_str()) {
            breaches.push(format!(
                "node `{}` has kind `{}`, which Nearbee does not check; no listing may declare it",
                node.id, node.kind
            ));
        }
        if node.interactive {
            breaches.push(format!(
                "node `{}` is interactive; Nearbee cannot list a playbook a human drives",
                node.id
            ));
        }
        if let Some(playbook) = &node.playbook_ref {
            breaches.push(format!(
                "node `{}` binds playbook `{}`; Nearbee lists a single playbook",
                node.id, playbook
            ));
        }
    }
    let mut attested_breaches: Vec<String> = Vec::new();
    // An attested listing runs script nodes only: its model calls are
    // Nearbee's own direct and signed calls (S6 design 4.1).
    if tier == Tier::Attested {
        for node in &facts.nodes {
            if AGENT_NODE_KINDS.contains(&node.kind.as_str()) {
                attested_breaches.push(format!(
                    "node `{}` is an `{}` node, which runs an agent; attested listings use Nearbee's direct calls only",
                    node.id, node.kind
                ));
            }
        }
    }
    let check_executor = |breaches: &mut Vec<String>, executor: &ExecutorFact, profile: &str| {
        if executor.agent == "opencode" {
            breaches.push(format!("profile `{profile}`: {OPENCODE_BREACH}"));
        } else {
            breaches.push(format!(
                "profile `{profile}`: {} is not a verified attested route; attested listings use Nearbee's direct calls only",
                executor.agent
            ));
        }
        if !model_ok(catalog, &executor.model) {
            breaches.push(format!(
                "profile `{}`: executor `{}` on model `{}` is not a NEAR AI Cloud TEE model in the catalog",
                profile, executor.agent, executor.model
            ));
        }
    };
    for reference in usable_references(facts) {
        let fact = match profile_fact(facts, &reference) {
            Some(fact) => fact,
            None => {
                let reason = facts
                    .profiles
                    .iter()
                    .find(|(r, _)| *r == reference)
                    .and_then(|(_, f)| match f {
                        Known::Unknown(reason) => Some(reason.clone()),
                        Known::Yes(_) => None,
                    })
                    .unwrap_or_else(|| "the apb instance does not provide it".to_string());
                breaches.push(format!(
                    "profile `{}` (scope `{}`) is unknown: {}",
                    reference.name, reference.scope, reason
                ));
                continue;
            }
        };
        let name = format!("{} ({})", reference.name, reference.scope);
        if tier == Tier::Attested {
            check_executor(&mut attested_breaches, &fact.executor, &name);
            for executor in &fact.fallbacks {
                check_executor(&mut attested_breaches, executor, &name);
            }
            for (_, executor) in &fact.tiers {
                check_executor(&mut attested_breaches, executor, &name);
            }
            if !fact.skills.is_empty() {
                attested_breaches.push(format!(
                    "profile `{}` declares skills {:?}; an attested listing runs no skills",
                    name, fact.skills
                ));
            }
            if fact.environment != "minimal" {
                attested_breaches.push(format!(
                    "profile `{}` uses environment `{}`; an attested listing runs `minimal`",
                    name, fact.environment
                ));
            }
            if fact.env_policy != "allowlist" {
                attested_breaches.push(format!(
                    "profile `{}` uses process environment policy `{}`; an attested listing uses an allowlist",
                    name, fact.env_policy
                ));
            }
            for entry in &fact.env_allow {
                if !ATTESTED_ENV_ALLOWED.contains(&entry.as_str()) {
                    attested_breaches.push(format!(
                        "profile `{}` allows process environment variable `{}`; an attested listing allows only {}",
                        name, entry, ATTESTED_ENV_ALLOWED[0]
                    ));
                }
            }
        }
    }
    // Direct models (spec `privacy-tiers`, Direct models): attested needs
    // catalog `nearai` models, plain catalog models.
    for model in &facts.direct_models {
        if model.starts_with("nearai/") {
            breaches.push(format!(
                "direct model `{model}` carries the nearai/ prefix; list the bare catalog id"
            ));
        } else if tier == Tier::Attested && !catalog.is_nearai(model) {
            attested_breaches.push(format!(
                "direct model `{model}` is not a NEAR AI Cloud TEE model in the catalog; an attested listing calls only those"
            ));
        } else if catalog.owned_by(model).is_none() {
            breaches.push(format!(
                "direct model `{model}` is not in the NEAR AI Cloud catalog snapshot"
            ));
        }
    }
    for model in &facts.signed_models {
        if !facts.direct_models.contains(model) {
            breaches.push(format!(
                "signed model `{model}` is not one of the direct models"
            ));
        }
    }
    if tier == Tier::Attested {
        for node in &facts.nodes {
            for connector in &node.connectors {
                attested_breaches.push(format!(
                    "node `{}` binds connector `{}`; an attested listing binds no connector",
                    node.id, connector
                ));
            }
        }
        match &facts.routes {
            Known::Yes(routes) => {
                let nearai = routes.iter().find(|r| r.name == "nearai");
                match nearai {
                    Some(route) => {
                        if route.base_url != NEARAI_BASE_URL {
                            attested_breaches.push(format!(
                                "the `nearai` provider route points at `{}`; an attested listing requires `{}`",
                                route.base_url, NEARAI_BASE_URL
                            ));
                        }
                        if route.api_key_ref != NEARAI_API_KEY_REF {
                            attested_breaches.push(format!(
                                "the `nearai` provider reads its key from `{}`; an attested listing requires `{}`",
                                route.api_key_ref, NEARAI_API_KEY_REF
                            ));
                        }
                        for entry in &route.model_overrides {
                            attested_breaches.push(format!(
                                "the `nearai` provider aliases or overrides model `{}`; an attested listing calls catalog ids as they are",
                                entry
                            ));
                        }
                    }
                    None => attested_breaches.push(
                        "no `nearai` provider route is configured; an attested listing requires one"
                            .to_string(),
                    ),
                }
            }
            Known::Unknown(reason) => attested_breaches.push(format!(
                "the model routes are unknown: {}; an attested listing requires them",
                reason
            )),
        }
        match &facts.decisions_enabled {
            Known::Yes(true) => attested_breaches.push(
                "decisions are enabled; the scope check would send the brief to a decision provider"
                    .to_string(),
            ),
            Known::Yes(false) | Known::Unknown(_) => {}
        }
        if facts.compaction_enabled == Known::Yes(true) {
            attested_breaches
                .push("context compaction is enabled; an attested listing requires it off".into());
        }
    }
    // The statement states both settings, so an unknown one refuses every
    // tier (spec `privacy-tiers`, Structural checks).
    if let Known::Unknown(reason) = &facts.decisions_enabled {
        breaches.push(format!(
            "the decisions setting is unknown ({}); the data statement must state it",
            reason
        ));
    }
    if let Known::Unknown(reason) = &facts.compaction_enabled {
        breaches.push(format!(
            "the compaction setting is unknown ({}); the data statement must state it",
            reason
        ));
    }
    // One line per distinct breach: an executor and its fallbacks on the
    // same agent say the same thing.
    let mut seen = std::collections::BTreeSet::new();
    attested_breaches.retain(|b| seen.insert(b.clone()));
    breaches.append(&mut attested_breaches);
    if breaches.is_empty() {
        Ok(())
    } else {
        Err(breaches)
    }
}

/// The sorted, distinct model ids (without the `nearai/` prefix) of every
/// usable profile's executor, fallbacks and tiers (design 3), and the
/// listing's direct models (design 5.4).
pub fn evidence_models(facts: &ExecutionFacts) -> Vec<String> {
    let mut models: Vec<String> = facts
        .direct_models
        .iter()
        .map(|m| strip_nearai(m).to_string())
        .collect();
    for reference in usable_references(facts) {
        if let Some(fact) = profile_fact(facts, &reference) {
            models.push(strip_nearai(&fact.executor.model).to_string());
            for executor in &fact.fallbacks {
                models.push(strip_nearai(&executor.model).to_string());
            }
            for (_, executor) in &fact.tiers {
                models.push(strip_nearai(&executor.model).to_string());
            }
        }
    }
    models.sort();
    models.dedup();
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEE: &str = "nearai/Qwen/Qwen3.8-27B";

    fn profile_ref(name: &str, scope: &str) -> ProfileRef {
        ProfileRef {
            name: name.to_string(),
            scope: scope.to_string(),
        }
    }

    fn executor(agent: &str, model: &str) -> ExecutorFact {
        ExecutorFact {
            agent: agent.to_string(),
            model: model.to_string(),
        }
    }

    /// A profile fact built like the shipped profiles: `opencode` on a
    /// TEE model, minimal environment, `NEARAI_API_KEY` allowlist.
    fn attested_profile(reference: ProfileRef) -> (ProfileRef, Known<ProfileFact>) {
        (
            reference.clone(),
            Known::Yes(ProfileFact {
                reference,
                executor: executor("opencode", TEE),
                fallbacks: vec![],
                tiers: vec![],
                environment: "minimal".to_string(),
                skills: vec![],
                env_policy: "allowlist".to_string(),
                env_allow: vec!["NEARAI_API_KEY".to_string()],
            }),
        )
    }

    fn node(id: &str, kind: &str) -> NodeFact {
        NodeFact {
            id: id.to_string(),
            kind: kind.to_string(),
            interactive: false,
            profile: None,
            connectors: vec![],
            playbook_ref: None,
        }
    }

    fn base_facts() -> ExecutionFacts {
        ExecutionFacts {
            playbook_id: "repo-audit".to_string(),
            default_profile: None,
            nodes: vec![node("start", "start"), node("finish", "finish")],
            profiles: vec![],
            routes: Known::Yes(vec![]),
            decisions_enabled: Known::Yes(false),
            compaction_enabled: Known::Yes(false),
            direct_models: Vec::new(),
            signed_models: Vec::new(),
            sandbox: None,
        }
    }

    fn catalog() -> Catalog {
        Catalog::embedded()
            .expect("the checked-in catalog parses")
            .clone()
    }

    fn with_direct(facts: ExecutionFacts, direct: &[&str], signed: &[&str]) -> ExecutionFacts {
        let d: Vec<String> = direct.iter().map(|s| s.to_string()).collect();
        let s: Vec<String> = signed.iter().map(|s| s.to_string()).collect();
        facts.with_direct_models(&d, &s)
    }

    /// Attested refuses a direct model that is not `nearai`; plain
    /// refuses one absent from the catalog and accepts any catalog model
    /// (task 3.1).
    #[test]
    fn direct_models_follow_the_tier() {
        let (facts, catalog) = script_only_setup();
        let ok = with_direct(
            facts.clone(),
            &["Qwen/Qwen3-VL-30B-A3B-Instruct", "openai/privacy-filter"],
            &[],
        );
        assert_eq!(check(Tier::Attested, &ok, &catalog), Ok(()));
        let bad = with_direct(facts.clone(), &["anthropic/claude-sonnet-5"], &[]);
        let err = check(Tier::Attested, &bad, &catalog)
            .unwrap_err()
            .join("\n");
        assert!(err.contains("anthropic/claude-sonnet-5"), "{err}");
        let plain = with_direct(base_facts(), &["anthropic/claude-sonnet-5"], &[]);
        assert_eq!(check(Tier::Plain, &plain, &catalog), Ok(()));
        let off = with_direct(base_facts(), &["no-such/model"], &[]);
        let err = check(Tier::Plain, &off, &catalog).unwrap_err().join("\n");
        assert!(err.contains("no-such/model"), "{err}");
        let unlisted = with_direct(
            base_facts(),
            &["openai/privacy-filter"],
            &["Qwen/Qwen3.8-27B"],
        );
        let err = check(Tier::Plain, &unlisted, &catalog)
            .unwrap_err()
            .join("\n");
        assert!(err.contains("Qwen/Qwen3.8-27B"), "{err}");
    }

    /// The evidence set includes the direct models (task 3.1).
    #[test]
    fn evidence_models_include_direct_models() {
        let (facts, _) = attested_setup();
        let before = evidence_models(&facts);
        let facts = with_direct(facts, &["openai/privacy-filter"], &[]);
        let after = evidence_models(&facts);
        assert!(after.contains(&"openai/privacy-filter".to_string()));
        assert_eq!(after.len(), before.len() + 1);
    }

    /// The S4 attested profile (`opencode` on a TEE model, correct route,
    /// no side channels) is now refused: the opencode agent runs in a
    /// shared background service (S6 design 4).
    #[test]
    fn an_attested_opencode_profile_is_refused_as_a_shared_service() {
        let (facts, catalog) = attested_setup();
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("opencode is refused");
        assert!(
            breaches.iter().any(|b| b.contains("nearbee-near")
                && b.contains("shared background service")
                && b.contains("without --standalone")
                && b.contains("attested listings use Nearbee's direct calls only")),
            "{breaches:?}"
        );
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));
    }

    /// An `agent_task` or `prompt` node refuses an attested listing even
    /// with no profile and no default profile, naming the node.
    #[test]
    fn an_agent_node_without_a_profile_refuses_an_attested_listing() {
        for kind in ["agent_task", "prompt"] {
            let (mut facts, catalog) = script_only_setup();
            facts.nodes.push(node("review", kind));
            let breaches =
                check(Tier::Attested, &facts, &catalog).expect_err("an agent node is refused");
            assert!(
                breaches
                    .iter()
                    .any(|b| b.contains("`review`") && b.contains(kind)),
                "{kind}: {breaches:?}"
            );
            assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()), "{kind}");
        }
    }

    /// A script-only attested playbook whose direct models are catalog
    /// `nearai` models passes.
    #[test]
    fn a_script_only_attested_playbook_passes() {
        let (facts, catalog) = script_only_setup();
        let facts = with_direct(
            facts,
            &["deepseek-ai/DeepSeek-V4.1-Flash-1017"],
            &["deepseek-ai/DeepSeek-V4.1-Flash-1017"],
        );
        assert_eq!(check(Tier::Attested, &facts, &catalog), Ok(()));
    }

    /// Every agent breach of an attested listing, for the S4 scenarios
    /// that now end refused by the agent rule.
    fn agent_refused(breaches: &[String]) -> bool {
        breaches
            .iter()
            .any(|b| b.contains("attested listings use Nearbee's direct calls only"))
    }

    /// A passing `plain` case built like the shipped profiles (task 2.2):
    /// unknown routes and an off-catalog model do not matter for `plain`.
    #[test]
    fn a_clean_plain_facts_passes() {
        let catalog = catalog();
        let mut facts = base_facts();
        facts.default_profile = Some(profile_ref("nearbee-near", "project"));
        let mut profile = attested_profile(profile_ref("nearbee-near", "project"));
        if let Known::Yes(fact) = &mut profile.1 {
            fact.executor = executor("claude", "claude-fable-5");
        }
        facts.profiles = vec![profile];
        facts.routes = Known::Unknown("opencode.json is missing".to_string());
        facts.decisions_enabled = Known::Yes(true);
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));
    }

    /// Unknown statement facts (spec `privacy-tiers`, Structural checks):
    /// the statement states decisions and compaction, so either unknown
    /// refuses a `plain` listing too, and the refusal names the setting.
    #[test]
    fn an_unknown_decisions_or_compaction_setting_refuses_a_plain_listing() {
        let catalog = catalog();
        let mut facts = base_facts();
        facts.decisions_enabled = Known::Unknown("config absent".to_string());
        let breaches = check(Tier::Plain, &facts, &catalog).expect_err("decisions unknown");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("decisions setting is unknown")),
            "{breaches:?}"
        );
        facts.decisions_enabled = Known::Yes(false);
        facts.compaction_enabled = Known::Unknown("apb version 1.0.0-beta.7".to_string());
        let breaches = check(Tier::Plain, &facts, &catalog).expect_err("compaction unknown");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("compaction setting is unknown")),
            "{breaches:?}"
        );
    }

    /// Review gate (spec `privacy-tiers`, Structural checks): a
    /// `human_review` node refuses the listing naming the node, whatever
    /// its tier.
    #[test]
    fn a_human_review_node_is_refused_for_both_tiers() {
        let catalog = catalog();
        for tier in [Tier::Plain, Tier::Attested] {
            let mut facts = base_facts();
            facts.nodes.push(node("gate", "human_review"));
            let breaches = check(tier, &facts, &catalog).expect_err("a review gate is refused");
            assert!(
                breaches
                    .iter()
                    .any(|b| b.contains("gate") && b.contains("human_review")),
                "the refusal names the node and its kind: {breaches:?}"
            );
        }
    }

    /// Interactive agent task (spec `privacy-tiers`, Structural checks).
    #[test]
    fn an_interactive_node_is_refused() {
        let catalog = catalog();
        let mut facts = base_facts();
        let mut task = node("task", "agent_task");
        task.interactive = true;
        facts.nodes.push(task);
        for tier in [Tier::Plain, Tier::Attested] {
            let breaches =
                check(tier, &facts, &catalog).expect_err("an interactive node is refused");
            assert!(breaches.iter().any(|b| b.contains("`task`")));
        }
    }

    /// Unknown node kind (design 3: any kind outside the known five is a
    /// breach, `human_review` named).
    #[test]
    fn an_unknown_node_kind_is_refused() {
        let catalog = catalog();
        let mut facts = base_facts();
        facts.nodes.push(node("wait", "wait_for_webhook"));
        let breaches =
            check(Tier::Plain, &facts, &catalog).expect_err("an unknown kind is refused");
        assert!(breaches.iter().any(|b| b.contains("wait_for_webhook")));
    }

    /// Sub-playbook binding (spec `privacy-tiers`, Structural checks).
    #[test]
    fn a_playbook_reference_is_refused() {
        let catalog = catalog();
        let mut facts = base_facts();
        let mut task = node("task", "agent_task");
        task.playbook_ref = Some("other-playbook".to_string());
        facts.nodes.push(task);
        for tier in [Tier::Plain, Tier::Attested] {
            let breaches = check(tier, &facts, &catalog).expect_err("a binding is refused");
            assert!(
                breaches
                    .iter()
                    .any(|b| b.contains("other-playbook") && b.contains("`task`")),
                "the refusal names the node and the playbook: {breaches:?}"
            );
        }
    }

    /// Scoped reference (spec `privacy-tiers`, Execution facts): a node
    /// naming `{name: nearbee-near, scope: global}` is checked against
    /// the global profile, not against a project profile of the same name.
    #[test]
    fn a_scoped_reference_uses_its_own_scope() {
        let catalog = catalog();
        let mut facts = base_facts();
        let mut task = node("task", "agent_task");
        task.profile = Some(profile_ref("nearbee-near", "global"));
        facts.nodes.push(task);
        // The project profile of the same name is unknown; only the global
        // one is usable, so the check must not fail.
        facts.profiles = vec![attested_profile(profile_ref("nearbee-near", "global"))];
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));

        // Without the global profile the same node is refused.
        facts.profiles = vec![attested_profile(profile_ref("nearbee-near", "project"))];
        let breaches =
            check(Tier::Plain, &facts, &catalog).expect_err("the global profile is missing");
        assert!(
            breaches.iter().any(|b| b.contains("global")),
            "the refusal names the scope: {breaches:?}"
        );
    }

    /// Unknown profile (spec `privacy-tiers`, Execution facts and
    /// Structural checks): an unreadable profile refuses every tier.
    #[test]
    fn an_unknown_profile_is_refused_for_both_tiers() {
        let catalog = catalog();
        let mut facts = base_facts();
        facts.default_profile = Some(profile_ref("nearbee-near", "project"));
        facts.profiles = vec![(
            profile_ref("nearbee-near", "project"),
            Known::Unknown("404".to_string()),
        )];
        for tier in [Tier::Plain, Tier::Attested] {
            let breaches =
                check(tier, &facts, &catalog).expect_err("an unknown profile is refused");
            assert!(
                breaches
                    .iter()
                    .any(|b| b.contains("nearbee-near") && b.contains("404")),
                "the refusal names the profile and the reason: {breaches:?}"
            );
        }
    }

    /// An attested playbook of script nodes only, with the correct route.
    fn script_only_setup() -> (ExecutionFacts, Catalog) {
        let (mut facts, catalog) = attested_setup();
        facts.default_profile = None;
        facts.profiles = vec![];
        facts.nodes.push(node("review", "script"));
        (facts, catalog)
    }

    fn attested_setup() -> (ExecutionFacts, Catalog) {
        let catalog = catalog();
        let mut facts = base_facts();
        facts.default_profile = Some(profile_ref("nearbee-near", "project"));
        facts.profiles = vec![attested_profile(profile_ref("nearbee-near", "project"))];
        facts.routes = Known::Yes(vec![ProviderRoute {
            name: "nearai".to_string(),
            base_url: NEARAI_BASE_URL.to_string(),
            api_key_ref: NEARAI_API_KEY_REF.to_string(),
            model_overrides: vec![],
        }]);
        (facts, catalog)
    }

    /// Incognito fallback (spec `privacy-tiers`, Attested model route):
    /// a fallback the catalog owns as `anthropic` refuses the listing
    /// naming the profile and the model.
    #[test]
    fn an_anthropic_fallback_is_refused() {
        let (mut facts, catalog) = attested_setup();
        let fact = facts.profiles.first_mut().expect("profile present");
        if let Known::Yes(profile) = &mut fact.1 {
            profile.fallbacks = vec![executor("claude", "anthropic/claude-fable-5")];
        }
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("the fallback is refused");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("nearbee-near") && b.contains("anthropic/claude-fable-5")),
            "the refusal names the profile and the model: {breaches:?}"
        );
        assert!(agent_refused(&breaches), "{breaches:?}");
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));
    }

    /// Agent other than `opencode` (spec `privacy-tiers`, Attested model
    /// route): a TEE model run by another agent refuses the listing, for
    /// the executor, a fallback and a tier alike, naming the profile.
    #[test]
    fn an_agent_other_than_opencode_is_refused() {
        type Mutate = fn(&mut ProfileFact);
        let cases: [(&str, Mutate); 3] = [
            ("executor", |p| p.executor = executor("claude", TEE)),
            ("fallback", |p| p.fallbacks = vec![executor("claude", TEE)]),
            ("tier", |p| {
                p.tiers = vec![("long".to_string(), executor("codex", TEE))]
            }),
        ];
        for (what, mutate) in cases {
            let (mut facts, catalog) = attested_setup();
            if let Known::Yes(profile) = &mut facts.profiles[0].1 {
                mutate(profile);
            }
            let breaches = check(Tier::Attested, &facts, &catalog)
                .expect_err("a non-opencode agent is refused");
            assert!(
                breaches.iter().any(|b| b.contains("nearbee-near")
                    && b.contains("is not a verified attested route")),
                "{what}: the refusal names the profile and the agent: {breaches:?}"
            );
            assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()), "{what}");
        }
    }

    /// Wrong route (spec `privacy-tiers`, Attested model route).
    #[test]
    fn a_wrong_nearai_route_is_refused() {
        let (mut facts, catalog) = attested_setup();
        if let Known::Yes(routes) = &mut facts.routes {
            routes[0].base_url = "http://localhost:9999/v1".to_string();
        }
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("the route is refused");
        assert!(agent_refused(&breaches), "{breaches:?}");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("http://localhost:9999/v1")),
            "the refusal names the route: {breaches:?}"
        );
        // A missing `nearai` route is refused as well.
        if let Known::Yes(routes) = &mut facts.routes {
            routes.clear();
        }
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("no route is refused");
        assert!(breaches.iter().any(|b| b.contains("nearai")));
    }

    /// Key reference (spec `privacy-tiers`, Attested model route: the key
    /// is read from `NEARAI_API_KEY`): any other `apiKey` refuses.
    #[test]
    fn a_nearai_key_reference_other_than_the_env_variable_is_refused() {
        for bad in ["sk-inline-key", "{env:OTHER_KEY}", ""] {
            let (mut facts, catalog) = attested_setup();
            if let Known::Yes(routes) = &mut facts.routes {
                routes[0].api_key_ref = bad.to_string();
            }
            let breaches =
                check(Tier::Attested, &facts, &catalog).expect_err("the key reference is refused");
            assert!(
                breaches
                    .iter()
                    .any(|b| b.contains("reads its key from") && b.contains(NEARAI_API_KEY_REF)),
                "{bad:?}: {breaches:?}"
            );
        }
    }

    /// Model alias (spec `privacy-tiers`, Attested model route): a model
    /// entry that maps a catalog id to another model or overrides its
    /// options refuses the listing, naming the entry.
    #[test]
    fn a_nearai_model_alias_or_override_is_refused() {
        let (mut facts, catalog) = attested_setup();
        if let Known::Yes(routes) = &mut facts.routes {
            routes[0].model_overrides = vec!["Qwen/Qwen3.8-27B: id".to_string()];
        }
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("the alias is refused");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("aliases or overrides") && b.contains("Qwen/Qwen3.8-27B: id")),
            "{breaches:?}"
        );
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));
    }

    /// OpenCode override as unknown routes (spec `privacy-tiers`):
    /// unknown routes refuse the attested listing.
    #[test]
    fn unknown_routes_refuse_the_attested_listing() {
        let (mut facts, catalog) = attested_setup();
        facts.routes = Known::Unknown("the instance sets OPENCODE_CONFIG".to_string());
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("unknown routes are refused");
        assert!(agent_refused(&breaches), "{breaches:?}");
        assert!(breaches.iter().any(|b| b.contains("OPENCODE_CONFIG")));
    }

    /// Unknown model (spec `privacy-tiers`, Attested model route): a
    /// `nearai/` model the snapshot does not list refuses the listing.
    #[test]
    fn a_model_not_in_the_catalog_is_refused() {
        let (mut facts, catalog) = attested_setup();
        let fact = facts.profiles.first_mut().expect("profile present");
        if let Known::Yes(profile) = &mut fact.1 {
            profile.executor = executor("opencode", "nearai/some-new-model");
        }
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("the model is refused");
        assert!(agent_refused(&breaches), "{breaches:?}");
        assert!(
            breaches.iter().any(|b| b.contains("nearai/some-new-model")),
            "the refusal names the model: {breaches:?}"
        );
    }

    /// Skills (spec `privacy-tiers`, Attested side channels).
    #[test]
    fn a_profile_with_skills_is_refused() {
        let (mut facts, catalog) = attested_setup();
        let fact = facts.profiles.first_mut().expect("profile present");
        if let Known::Yes(profile) = &mut fact.1 {
            profile.skills = vec!["deploy".to_string()];
        }
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("skills are refused");
        assert!(breaches.iter().any(|b| b.contains("deploy")));
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));
    }

    /// Environment (spec `privacy-tiers`, Attested side channels).
    #[test]
    fn an_environment_other_than_minimal_is_refused() {
        let (mut facts, catalog) = attested_setup();
        let fact = facts.profiles.first_mut().expect("profile present");
        if let Known::Yes(profile) = &mut fact.1 {
            profile.environment = "full".to_string();
        }
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("the environment is refused");
        assert!(breaches.iter().any(|b| b.contains("full")));
    }

    /// Env allowlist (spec `privacy-tiers`, Attested side channels): the
    /// policy must be an allowlist naming at most `NEARAI_API_KEY`.
    #[test]
    fn a_process_environment_outside_the_allowlist_is_refused() {
        let (mut facts, catalog) = attested_setup();
        let fact = facts.profiles.first_mut().expect("profile present");
        if let Known::Yes(profile) = &mut fact.1 {
            profile.env_policy = "inherit".to_string();
        }
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("the policy is refused");
        assert!(breaches.iter().any(|b| b.contains("inherit")));

        let fact = facts.profiles.first_mut().expect("profile present");
        if let Known::Yes(profile) = &mut fact.1 {
            profile.env_policy = "allowlist".to_string();
            profile.env_allow = vec![
                "NEARAI_API_KEY".to_string(),
                "AWS_SECRET_ACCESS_KEY".to_string(),
            ];
        }
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("the variable is refused");
        assert!(
            breaches.iter().any(|b| b.contains("AWS_SECRET_ACCESS_KEY")),
            "the refusal names the variable: {breaches:?}"
        );
    }

    /// Connector (spec `privacy-tiers`, Attested side channels).
    #[test]
    fn a_connector_binding_is_refused() {
        let (mut facts, catalog) = attested_setup();
        let mut task = node("task", "agent_task");
        task.connectors = vec!["github".to_string()];
        facts.nodes.push(task);
        let breaches = check(Tier::Attested, &facts, &catalog).expect_err("a connector is refused");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("github") && b.contains("`task`")),
            "the refusal names the node and the connector: {breaches:?}"
        );
        assert_eq!(check(Tier::Plain, &facts, &catalog), Ok(()));
    }

    /// Decisions on and unknown (spec `privacy-tiers`, Attested side
    /// channels): the scope check would send the brief to a provider.
    #[test]
    fn decisions_on_or_unknown_are_refused() {
        let (mut facts, catalog) = attested_setup();
        facts.decisions_enabled = Known::Yes(true);
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("decisions on are refused");
        assert!(
            breaches.iter().any(|b| b.contains("decision provider")),
            "the refusal says the brief would be sent: {breaches:?}"
        );
        facts.decisions_enabled = Known::Unknown("config absent".to_string());
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("decisions unknown are refused");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("decisions setting is unknown"))
        );
    }

    /// Compaction unknown (spec `privacy-tiers`, Attested side channels).
    #[test]
    fn compaction_unknown_or_on_is_refused() {
        let (mut facts, catalog) = attested_setup();
        facts.compaction_enabled = Known::Unknown("apb version 1.0.0-beta.7".to_string());
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("compaction unknown is refused");
        assert!(
            breaches
                .iter()
                .any(|b| b.contains("compaction setting is unknown"))
        );
        facts.compaction_enabled = Known::Yes(true);
        let breaches =
            check(Tier::Attested, &facts, &catalog).expect_err("compaction on is refused");
        assert!(breaches.iter().any(|b| b.contains("compaction is enabled")));
    }

    /// `evidence_models` returns the stripped ids of every usable
    /// profile's executor, fallbacks and tiers, sorted and deduplicated
    /// (design 3).
    #[test]
    fn evidence_models_are_sorted_distinct_and_stripped() {
        let mut facts = base_facts();
        facts.default_profile = Some(profile_ref("nearbee-near", "project"));
        let mut profile = attested_profile(profile_ref("nearbee-near", "project"));
        if let Known::Yes(fact) = &mut profile.1 {
            fact.fallbacks = vec![
                executor("opencode", "nearai/Qwen/Qwen3.6-35B-A3B-FP8"),
                executor("opencode", TEE),
            ];
            fact.tiers = vec![("long".to_string(), executor("opencode", TEE))];
        }
        facts.profiles = vec![profile];
        assert_eq!(
            evidence_models(&facts),
            vec![
                "Qwen/Qwen3.6-35B-A3B-FP8".to_string(),
                "Qwen/Qwen3.8-27B".to_string()
            ]
        );
    }

    /// `evidence_models` skips unknown profiles (they are unusable).
    #[test]
    fn evidence_models_skip_unknown_profiles() {
        let mut facts = base_facts();
        facts.profiles = vec![(
            profile_ref("other", "project"),
            Known::Unknown("404".to_string()),
        )];
        assert!(evidence_models(&facts).is_empty());
    }
}
