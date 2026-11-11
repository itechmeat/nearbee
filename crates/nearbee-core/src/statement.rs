//! The data statement: what a listing tells the buyer about its models,
//! channels and retention, generated only from the tier and the checked
//! execution facts, plus the fingerprint of those facts and the drift of
//! a published record against them (design 4.1 to 4.3, spec
//! `data-statement`).
//!
//! The text is deterministic: facts in a fixed order, no timestamps,
//! prices, run ids or machine paths, so the same facts always give the
//! same bytes. The fingerprint keeps the declaration order of fallbacks
//! and tiers, so a fact change that leaves the text unchanged still
//! changes the fingerprint.

use crate::catalog::Catalog;
use crate::journal::sha256_hex;
use crate::listing::{ResolvedListing, Tier};
use crate::tiers::{self, ExecutionFacts, Known, ProfileFact};

/// The statement format a record must carry to be serveable.
pub const STATEMENT_FORMAT: u32 = 1;

/// The heading every statement starts with (spec `data-statement`,
/// Generated content).
pub const HEADING: &str = "## Data handling";

/// Render the statement for a listing of `tier` built from `facts`,
/// with the local retention of `retention_days` (design 4.1).
pub fn render(tier: Tier, facts: &ExecutionFacts, retention_days: u32) -> String {
    let mut out = String::from(HEADING);
    out.push_str("\n\n");
    out.push_str(&models_section(facts));
    out.push_str(&direct_section(facts));
    out.push('\n');
    out.push_str(&channels_section(facts));
    out.push_str(&run_place_section(tier));
    out.push_str(&market_keeps_section());
    out.push_str(&retention_section(retention_days, facts));
    out
}

/// `sha256:` plus the hex SHA-256 of a canonical JSON encoding of the
/// tier and the facts, keeping the declaration order of fallbacks and
/// tiers (design 4.2).
pub fn fingerprint(tier: Tier, facts: &ExecutionFacts) -> String {
    let bytes = serde_json::to_vec(&(tier, facts)).expect("the facts always serialize");
    format!("sha256:{}", sha256_hex(&bytes))
}

/// Why the record no longer matches the current facts and retention, or
/// `None` when it is serveable, the check passes, the fingerprint
/// matches and the recorded text equals the rendered one (design 4.3,
/// spec `data-statement`, Drift).
pub fn drift(
    record: &ResolvedListing,
    facts: &ExecutionFacts,
    retention_days: u32,
) -> Option<String> {
    // The runner never reports the direct models: they come from the
    // record, as at `nearbee list` (design 5.1).
    let facts = &facts
        .clone()
        .with_direct_models(&record.direct_models, &record.signed_models);
    let mut reasons: Vec<String> = Vec::new();
    if !record.serveable() {
        if record.tier.is_none() {
            reasons.push("the record has no tier; run `nearbee list` again".to_string());
        }
        if record.fingerprint.is_none() {
            reasons.push("the record has no fingerprint; run `nearbee list` again".to_string());
        }
        if record.statement.is_none() {
            reasons.push("the record has no statement; run `nearbee list` again".to_string());
        }
        match (&record.statement, &record.statement_sha256) {
            (Some(s), Some(h)) if *h != sha256_hex(s.as_bytes()) => {
                reasons
                    .push("the recorded statement hash does not match the statement".to_string());
            }
            (Some(_), None) | (None, Some(_)) => {
                reasons.push(
                    "the record is missing the statement or its hash; run `nearbee list` again"
                        .to_string(),
                );
            }
            _ => {}
        }
        if record.statement_format != Some(STATEMENT_FORMAT) {
            reasons.push(format!(
                "the record carries statement format {:?}, not the supported format {STATEMENT_FORMAT}; run `nearbee list` again",
                record.statement_format
            ));
        }
    }
    let tier = record.tier;
    if let Some(tier) = tier {
        let catalog = Catalog::embedded();
        match catalog {
            Ok(catalog) => {
                if let Err(breaches) = tiers::check(tier, facts, catalog) {
                    reasons.push(format!("the tier check fails: {}", breaches.join("; ")));
                }
            }
            Err(err) => reasons.push(format!("the embedded model catalog is unreadable: {err}")),
        }
        if let Some(recorded) = &record.fingerprint {
            let current = fingerprint(tier, facts);
            if &current != recorded {
                reasons.push(format!(
                    "the execution facts changed since the listing was published (recorded fingerprint {recorded}, current {current})"
                ));
            }
        }
        if let Some(recorded) = &record.statement {
            let current = render(tier, facts, retention_days);
            if &current != recorded {
                reasons.push(
                    "the statement rendered from the current facts and retention differs from the recorded one".to_string(),
                );
            }
        }
    }
    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    }
}

/// One rendered model line: the wording of design 4.1 per agent and
/// model kind.
fn model_line(agent: &str, model: &str, checked_nearai: bool) -> String {
    if agent == "claude" {
        return format!(
            "- `claude`, model `{model}`: the owner's Claude subscription (Anthropic).\n"
        );
    }
    if agent == "codex" {
        return format!("- `codex`, model `{model}`: the owner's OpenAI subscription.\n");
    }
    if agent == "opencode" && checked_nearai {
        let id = model.strip_prefix("nearai/").unwrap_or(model);
        return format!(
            "- `opencode`, model `{id}`: NEAR AI Cloud Confidential TEE model `{id}`.\n"
        );
    }
    format!("- agent `{agent}`, model `{model}`.\n")
}

/// True when the model runs on a catalog `nearai` model, the same rule
/// the tier check applies (design 4.1: a checked `nearai/` model). An
/// unreadable catalog means nothing counts as checked, which is the
/// weaker claim and so the safe one.
fn checked_nearai(catalog: Option<&Catalog>, model: &str) -> bool {
    match catalog {
        Some(catalog) => model.starts_with("nearai/") && catalog.is_nearai(model),
        None => false,
    }
}

/// The profiles the listing uses, resolved like the tier check
/// (`tiers::usable_references`), with their facts; unknown profiles are
/// skipped, sorted by reference so the text is deterministic (spec
/// `data-statement`, Deterministic text).
fn used_profiles(facts: &ExecutionFacts) -> Vec<&ProfileFact> {
    let mut out = Vec::new();
    for reference in tiers::usable_references(facts) {
        if let Some((_, Known::Yes(fact))) = facts
            .profiles
            .iter()
            .find(|(r, f)| r == &reference && matches!(f, Known::Yes(_)))
        {
            out.push(fact);
        }
    }
    out
}

fn models_section(facts: &ExecutionFacts) -> String {
    let catalog = Catalog::embedded().ok();
    let mut out = String::from("### Models\n\n");
    let mut seen: Vec<(String, String)> = Vec::new();
    for fact in used_profiles(facts) {
        push_model(
            &mut out,
            &mut seen,
            &fact.executor.agent,
            &fact.executor.model,
            catalog,
        );
        // Fallbacks and tiers are listed sorted, so swapping two
        // fallbacks or reordering tiers leaves the text unchanged while
        // the fingerprint still moves (spec `data-statement`, Fallback
        // changed, text equal).
        let mut extras: Vec<(String, String)> = fact
            .fallbacks
            .iter()
            .map(|e| (&e.agent, &e.model))
            .chain(fact.tiers.iter().map(|(_, e)| (&e.agent, &e.model)))
            .map(|(a, m)| (a.clone(), m.clone()))
            .collect();
        extras.sort();
        extras.dedup();
        for (agent, model) in &extras {
            push_model(&mut out, &mut seen, agent, model, catalog);
        }
    }
    // A playbook of script steps only (agent B) names no agent model; a
    // listing with a profile never reaches this line, so S4 text stays.
    if seen.is_empty() {
        out.push_str("No agent step runs a model.\n");
    }
    out.push('\n');
    out
}

/// Add one model line unless the (agent, model) pair is already listed.
fn push_model(
    out: &mut String,
    seen: &mut Vec<(String, String)>,
    agent: &str,
    model: &str,
    catalog: Option<&Catalog>,
) {
    let key = (agent.to_string(), model.to_string());
    if seen.contains(&key) {
        return;
    }
    seen.push(key);
    out.push_str(&model_line(agent, model, checked_nearai(catalog, model)));
}

/// The direct model calls of Nearbee's commands (design 5.3); empty when
/// the listing has none, so an S4 statement is unchanged.
fn direct_section(facts: &ExecutionFacts) -> String {
    if facts.direct_models.is_empty() {
        return String::new();
    }
    let catalog = Catalog::embedded().ok();
    let mut out = String::from("### Direct model calls\n\n");
    out.push_str(
        "Nearbee's own commands in the playbook's script steps call these models directly on NEAR AI Cloud (`https://cloud-api.near.ai/v1`, no proxy), without an agent:\n\n",
    );
    for model in &facts.direct_models {
        let owner = catalog.and_then(|c| c.owned_by(model));
        match owner {
            Some("nearai") => out.push_str(&format!(
                "- `{model}`: NEAR AI Cloud Confidential TEE model `{model}`.\n"
            )),
            Some(owner) => out.push_str(&format!(
                "- `{model}`: NEAR AI Cloud model from `{owner}`, not a TEE model.\n"
            )),
            None => out.push_str(&format!("- `{model}`: not in the catalog snapshot.\n")),
        }
    }
    if !facts.signed_models.is_empty() {
        let names: Vec<String> = facts
            .signed_models
            .iter()
            .map(|m| format!("`{m}`"))
            .collect();
        out.push_str(&format!(
            "\nThe signed step: the call to {} carries a response signature from the model TEE (`provider_tee`), shown in the deliverable with a link to check it.\n",
            names.join(", ")
        ));
    }
    out.push('\n');
    out
}

fn channels_section(facts: &ExecutionFacts) -> String {
    let mut out = String::from("### Other channels\n\n");
    let mut connectors: Vec<&str> = facts
        .nodes
        .iter()
        .flat_map(|n| n.connectors.iter().map(|s| s.as_str()))
        .collect();
    connectors.sort();
    connectors.dedup();
    // An unknown setting is stated without its reason: the reason may
    // name a machine path, which the statement never prints (spec
    // `data-statement`, Deterministic text). The tier check refuses an
    // unknown setting for every tier anyway.
    let setting = |fact: &Known<bool>| match fact {
        Known::Yes(true) => "on",
        Known::Yes(false) => "off",
        Known::Unknown(_) => "unknown",
    };
    let decisions = setting(&facts.decisions_enabled);
    let compaction = setting(&facts.compaction_enabled);
    if connectors.is_empty() {
        out.push_str("No connector is bound.\n");
    } else {
        out.push_str(&format!(
            "Connectors bound: {}.\n",
            connectors
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push_str(&format!("Decisions are {decisions}.\n"));
    out.push_str(&format!("Compaction is {compaction}.\n"));
    let any_on = !connectors.is_empty()
        || facts.decisions_enabled == Known::Yes(true)
        || facts.compaction_enabled == Known::Yes(true);
    if any_on {
        out.push_str("These channels may send data to providers Nearbee has not validated.\n");
    }
    out.push('\n');
    out
}

fn run_place_section(tier: Tier) -> String {
    let mut out = String::from("### Where the run happens\n\n");
    if tier == Tier::Attested {
        out.push_str(
            "The model steps are configured to run on NEAR AI Cloud Confidential TEE models.\n",
        );
    }
    out.push_str(
        "The run happens on the owner's self-hosted machine, which can read the data. The agent steps run without a sandbox: reading files the owner's account can read and network access the bindings above do not declare are not prevented.\n\n",
    );
    out
}

fn market_keeps_section() -> String {
    let mut out = String::from("### What the market keeps\n\n");
    out.push_str(
        "The brief, thread messages and uploaded files stay with the market under its own policy. A file once used by a job cannot be deleted there, and its link works for anyone holding it.\n\n",
    );
    out
}

fn retention_section(retention_days: u32, facts: &ExecutionFacts) -> String {
    let mut out = String::from("### Local retention\n\n");
    out.push_str(&format!(
        "Nearbee deletes its own copies (the hire's worktree and its snapshots) {retention_days} days after the market reports acceptance; deletion is journaled. Not deleted by Nearbee: apb run directories, apb decision logs, executor caches.\n"
    ));
    // Every market run is an apb external-data run (S6 design 3.8); the
    // flag does not reach what an agent CLI keeps itself.
    out.push_str(
        "\nEvery run is an apb external-data run: apb keeps it out of what it learns from (corrections, memory, statistics, judge examples, discovery). ",
    );
    let mut agents: Vec<&str> = used_profiles(facts)
        .into_iter()
        .flat_map(|f| {
            std::iter::once(f.executor.agent.as_str())
                .chain(f.fallbacks.iter().map(|e| e.agent.as_str()))
                .chain(f.tiers.iter().map(|(_, e)| e.agent.as_str()))
        })
        .collect();
    agents.sort_unstable();
    agents.dedup();
    if agents.is_empty() {
        out.push_str("No agent CLI runs, so no agent session files are kept.\n");
    } else {
        out.push_str("The flag does not reach what an agent CLI keeps itself");
        if agents.contains(&"claude") {
            out.push_str(": `claude` keeps its session files under the owner's home directory");
        }
        out.push_str(".\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiers::{
        ExecutorFact, NEARAI_API_KEY_REF, NEARAI_BASE_URL, NodeFact, ProfileRef, ProviderRoute,
    };

    const TEE_A: &str = "nearai/Qwen/Qwen3.8-27B";
    const TEE_B: &str = "nearai/deepseek-ai/DeepSeek-V4.1-Flash-1017";

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
        }
    }

    /// A premium-like profile: `claude` on an incognito Anthropic model.
    fn premium_profile(reference: ProfileRef) -> (ProfileRef, Known<ProfileFact>) {
        (
            reference.clone(),
            Known::Yes(ProfileFact {
                reference,
                executor: executor("claude", "claude-fable-5"),
                fallbacks: vec![],
                tiers: vec![],
                environment: "minimal".to_string(),
                skills: vec![],
                env_policy: "allowlist".to_string(),
                env_allow: vec!["NEARAI_API_KEY".to_string()],
            }),
        )
    }

    /// A near-like profile: `opencode` on a TEE model.
    fn near_profile(reference: ProfileRef) -> (ProfileRef, Known<ProfileFact>) {
        (
            reference.clone(),
            Known::Yes(ProfileFact {
                reference,
                executor: executor("opencode", TEE_A),
                fallbacks: vec![],
                tiers: vec![],
                environment: "minimal".to_string(),
                skills: vec![],
                env_policy: "allowlist".to_string(),
                env_allow: vec!["NEARAI_API_KEY".to_string()],
            }),
        )
    }

    fn plain_premium_facts() -> ExecutionFacts {
        let mut facts = base_facts();
        facts.default_profile = Some(profile_ref("nearbee-own-premium", "project"));
        facts.profiles = vec![premium_profile(profile_ref(
            "nearbee-own-premium",
            "project",
        ))];
        facts
    }

    fn attested_facts() -> ExecutionFacts {
        let mut facts = base_facts();
        facts.default_profile = Some(profile_ref("nearbee-near", "project"));
        let mut profile = near_profile(profile_ref("nearbee-near", "project"));
        if let Known::Yes(fact) = &mut profile.1 {
            fact.fallbacks = vec![executor("opencode", TEE_B)];
        }
        facts.profiles = vec![profile];
        facts.routes = Known::Yes(vec![ProviderRoute {
            name: "nearai".to_string(),
            base_url: NEARAI_BASE_URL.to_string(),
            api_key_ref: NEARAI_API_KEY_REF.to_string(),
            model_overrides: vec![],
        }]);
        facts
    }

    /// Attested facts that pass the S6 tier check: script nodes only, the
    /// correct route, no profile (S6 design 4).
    fn script_attested_facts() -> ExecutionFacts {
        let mut facts = attested_facts();
        facts.default_profile = None;
        facts.profiles = vec![];
        facts.nodes.push(node("review", "script"));
        facts
    }

    const QWEN_VL: &str = "Qwen/Qwen3-VL-30B-A3B-Instruct";
    const FILTER: &str = "openai/privacy-filter";
    const DS: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";

    fn digest_facts() -> ExecutionFacts {
        attested_facts()
            .with_direct_models(&[QWEN_VL.into(), FILTER.into(), DS.into()], &[DS.into()])
    }

    /// The fingerprint S4 computed for `attested_facts` (recorded before
    /// the direct model fields existed, printed at the S4 commit): a
    /// listing without direct models
    /// keeps it (task 3.1).
    const S4_ATTESTED_FINGERPRINT: &str =
        "sha256:cdb3c2264d4aa3c8bc4b8152f321877d7747c7f6e34e807e9ed1247459984c68";

    #[test]
    fn a_listing_without_direct_models_keeps_the_s4_fingerprint() {
        assert_eq!(
            fingerprint(Tier::Attested, &attested_facts()),
            S4_ATTESTED_FINGERPRINT
        );
        assert_eq!(
            render(Tier::Attested, &attested_facts(), 7),
            render(
                Tier::Attested,
                &attested_facts().with_direct_models(&[], &[]),
                7
            )
        );
    }

    #[test]
    fn reordering_direct_models_moves_the_fingerprint() {
        let reordered = attested_facts()
            .with_direct_models(&[FILTER.into(), QWEN_VL.into(), DS.into()], &[DS.into()]);
        assert_ne!(
            fingerprint(Tier::Attested, &digest_facts()),
            fingerprint(Tier::Attested, &reordered)
        );
        assert_ne!(
            fingerprint(Tier::Attested, &digest_facts()),
            fingerprint(Tier::Attested, &attested_facts())
        );
    }

    /// Spec `data-statement`, Agent B statement.
    #[test]
    fn the_statement_names_direct_models_and_the_signed_step() {
        let text = render(Tier::Attested, &digest_facts(), 7);
        let section = text
            .split("### Direct model calls")
            .nth(1)
            .expect("the section is there");
        for m in [QWEN_VL, FILTER, DS] {
            assert!(
                section.contains(&format!("NEAR AI Cloud Confidential TEE model `{m}`")),
                "{text}"
            );
        }
        assert!(section.contains("The signed step"), "{text}");
        assert!(section.contains(&format!("`{DS}` carries a response signature")));
        let unsigned = attested_facts().with_direct_models(&[FILTER.into()], &[]);
        assert!(!render(Tier::Attested, &unsigned, 7).contains("signed step"));
    }

    /// The drift pass gets facts without direct models from the runner;
    /// a record with them is still clean (spec `data-statement`, No drift
    /// from direct models).
    #[test]
    fn the_drift_pass_adds_the_records_direct_models() {
        let facts = script_attested_facts()
            .with_direct_models(&[QWEN_VL.into(), FILTER.into(), DS.into()], &[DS.into()]);
        let mut rec = record(Tier::Attested, &facts);
        rec.direct_models = facts.direct_models.clone();
        rec.signed_models = facts.signed_models.clone();
        assert_eq!(drift(&rec, &script_attested_facts(), 7), None);
        // A record whose lists differ from the published facts drifts.
        rec.signed_models.clear();
        assert!(drift(&rec, &script_attested_facts(), 7).is_some());
    }

    fn record(tier: Tier, facts: &ExecutionFacts) -> ResolvedListing {
        let statement = render(tier, facts, 7);
        ResolvedListing {
            handle: "repo-audit-fast".to_owned(),
            agent_id: "ag_1".into(),
            playbook_id: "repo-audit-fast".into(),
            playbook_version: "1.0.0".into(),
            playbook_sha256: "ab".into(),
            price_usd: "0.50".into(),
            sla_seconds: 3600,
            scope: None,
            protection: None,
            limits: None,
            tier: Some(tier),
            intake_check: None,
            statement_sha256: Some(sha256_hex(statement.as_bytes())),
            statement: Some(statement),
            statement_format: Some(STATEMENT_FORMAT),
            fingerprint: Some(fingerprint(tier, facts)),
            evidence_models: Vec::new(),
            direct_models: Vec::new(),
            signed_models: Vec::new(),
            published_at: time::OffsetDateTime::from_unix_timestamp(0).unwrap(),
        }
    }

    /// Every statement says runs are apb external-data runs and what the
    /// flag does not reach: an agent CLI's own session files, named for
    /// `claude`, and none for a listing without agents (S6 design 3.8).
    #[test]
    fn the_statement_names_external_data_runs_and_what_they_do_not_reach() {
        let plain = render(Tier::Plain, &plain_premium_facts(), 7);
        assert!(plain.contains("apb external-data run"), "{plain}");
        assert!(
            plain.contains("`claude` keeps its session files under the owner's home directory"),
            "{plain}"
        );
        let attested = render(Tier::Attested, &script_attested_facts(), 7);
        assert!(attested.contains("apb external-data run"), "{attested}");
        assert!(attested.contains("No agent CLI runs"), "{attested}");
        assert!(!attested.contains("session files under"), "{attested}");
    }

    /// Scenario "Plain listing": the statement names the `claude` agent,
    /// the model and the owner's subscription, and makes no TEE claim.
    #[test]
    fn a_plain_statement_names_claude_and_makes_no_tee_claim() {
        let text = render(Tier::Plain, &plain_premium_facts(), 7);
        assert!(
            text.starts_with(HEADING),
            "the statement starts with the heading: {text}"
        );
        assert!(
            text.contains("`claude`") && text.contains("claude-fable-5"),
            "the agent and the model are named: {text}"
        );
        assert!(
            text.contains("the owner's Claude subscription (Anthropic)"),
            "the subscription is named: {text}"
        );
        assert!(
            !text.contains("TEE") && !text.contains("Confidential"),
            "no TEE claim is made: {text}"
        );
    }

    /// Scenario "Plain listing with channels on": the connector is
    /// named, decisions are said to be on, and the not-validated
    /// providers sentence appears.
    #[test]
    fn a_plain_statement_with_channels_names_them_and_the_risk() {
        let mut facts = plain_premium_facts();
        facts.nodes[1].connectors = vec!["github".to_string()];
        facts.decisions_enabled = Known::Yes(true);
        let text = render(Tier::Plain, &facts, 7);
        assert!(text.contains("`github`"), "the connector is named: {text}");
        assert!(
            text.contains("Decisions are on"),
            "decisions are on: {text}"
        );
        assert!(
            text.contains("These channels may send data to providers Nearbee has not validated."),
            "the risk sentence appears: {text}"
        );
    }

    /// Scenario "Attested listing": both TEE models are named in the
    /// executor-then-fallback order and the statement still says the
    /// owner's machine can read the data.
    #[test]
    fn an_attested_statement_names_both_tee_models_and_the_owner_machine() {
        let text = render(Tier::Attested, &attested_facts(), 7);
        let a = text
            .find("Qwen3.8-27B")
            .expect("the executor model is named");
        let b = text
            .find("DeepSeek-V4.1-Flash-1017")
            .expect("the fallback model is named");
        assert!(a < b, "the models are named in declaration order: {text}");
        assert!(
            text.contains("NEAR AI Cloud Confidential TEE model"),
            "the TEE wording appears: {text}"
        );
        assert!(
            text.contains("the owner's self-hosted machine, which can read the data"),
            "the owner machine sentence appears: {text}"
        );
        // What apb does not enforce for the agent step is stated as not
        // prevented (spec `data-statement`, Generated content).
        assert!(
            text.contains("The agent steps run without a sandbox: reading files the owner's account can read and network access the bindings above do not declare are not prevented."),
            "the no-sandbox sentence appears: {text}"
        );
    }

    /// Scenario "Retention days": the configured days and the list of
    /// what cleanup does not cover appear.
    #[test]
    fn the_statement_says_the_retention_days_and_what_is_not_covered() {
        let text = render(Tier::Plain, &plain_premium_facts(), 30);
        assert!(text.contains("30 days"), "the days appear: {text}");
        for not_covered in [
            "apb run directories",
            "apb decision logs",
            "executor caches",
        ] {
            assert!(
                text.contains(not_covered),
                "the not-covered list names {not_covered}: {text}"
            );
        }
    }

    /// Scenario "Same facts twice": equal facts give byte-identical
    /// text, whatever the call order.
    /// No machine path (spec `data-statement`, Deterministic text): an
    /// unknown setting whose reason names a file is stated as `unknown`,
    /// without the reason.
    #[test]
    fn an_unknown_setting_is_stated_without_its_reason() {
        let mut facts = plain_premium_facts();
        facts.decisions_enabled = Known::Unknown(
            "/home/owner/.local/share/nearbee/apb-workspace/.apb/config.yaml: decisions.enabled is not set"
                .to_string(),
        );
        facts.compaction_enabled = Known::Unknown("GET /api/health at /srv/apb".to_string());
        let text = render(Tier::Plain, &facts, 7);
        assert!(text.contains("Decisions are unknown.\n"), "{text}");
        assert!(text.contains("Compaction is unknown.\n"), "{text}");
        assert!(!text.contains("/home/"), "{text}");
        assert!(
            !text.contains("config.yaml") && !text.contains("/srv/"),
            "{text}"
        );
    }

    #[test]
    fn equal_facts_give_identical_bytes() {
        let a = render(Tier::Plain, &plain_premium_facts(), 7);
        let b = render(Tier::Plain, &plain_premium_facts(), 7);
        assert_eq!(a, b);
    }

    /// Scenario "Fallback changed, text equal": two fallbacks swapping
    /// leaves the text identical but moves the fingerprint; reordering
    /// the `tiers` map does the same.
    #[test]
    fn a_fallback_swap_keeps_the_text_and_moves_the_fingerprint() {
        let mut facts = attested_facts();
        if let Known::Yes(fact) = &mut facts.profiles[0].1 {
            fact.fallbacks = vec![executor("opencode", TEE_B), executor("opencode", TEE_A)];
        }
        let mut swapped = facts.clone();
        if let Known::Yes(fact) = &mut swapped.profiles[0].1 {
            fact.fallbacks.reverse();
        }
        assert_eq!(
            render(Tier::Attested, &facts, 7),
            render(Tier::Attested, &swapped, 7),
            "the text does not depend on the fallback order"
        );
        assert_ne!(
            fingerprint(Tier::Attested, &facts),
            fingerprint(Tier::Attested, &swapped),
            "the fingerprint keeps the declaration order"
        );

        let mut reordered = facts.clone();
        if let Known::Yes(fact) = &mut reordered.profiles[0].1 {
            fact.tiers = vec![
                ("opus".to_string(), executor("claude", "claude-opus-9")),
                ("haiku".to_string(), executor("claude", "claude-haiku-9")),
            ];
        }
        if let Known::Yes(fact) = &mut facts.profiles[0].1 {
            fact.tiers = vec![
                ("haiku".to_string(), executor("claude", "claude-haiku-9")),
                ("opus".to_string(), executor("claude", "claude-opus-9")),
            ];
        }
        assert_ne!(
            fingerprint(Tier::Attested, &facts),
            fingerprint(Tier::Attested, &reordered),
            "a tier reorder moves the fingerprint"
        );
    }

    /// Scenario "Old record" and Drift: a record missing a field, a
    /// corrupt statement hash, an unsupported format, a changed
    /// retention and a fingerprint mismatch each report a reason; a
    /// matching record reports `None`.
    #[test]
    fn drift_reports_every_mismatch_and_none_for_a_matching_record() {
        let facts = script_attested_facts();
        let good = record(Tier::Attested, &facts);
        assert_eq!(
            drift(&good, &facts, 7),
            None,
            "a matching record has no drift"
        );

        // A missing field, one at a time.
        let mut missing_tier = good.clone();
        missing_tier.tier = None;
        let reason = drift(&missing_tier, &facts, 7).expect("a record without a tier drifts");
        assert!(reason.contains("no tier"), "{reason}");

        let mut missing_fingerprint = good.clone();
        missing_fingerprint.fingerprint = None;
        let reason =
            drift(&missing_fingerprint, &facts, 7).expect("a record without a fingerprint drifts");
        assert!(reason.contains("no fingerprint"), "{reason}");

        let mut missing_statement = good.clone();
        missing_statement.statement = None;
        let reason =
            drift(&missing_statement, &facts, 7).expect("a record without a statement drifts");
        assert!(reason.contains("no statement"), "{reason}");

        // A corrupt statement hash.
        let mut corrupt = good.clone();
        corrupt.statement_sha256 = Some("deadbeef".to_string());
        let reason = drift(&corrupt, &facts, 7).expect("a corrupt hash drifts");
        assert!(reason.contains("hash does not match"), "{reason}");

        // An unsupported statement format.
        let mut old_format = good.clone();
        old_format.statement_format = Some(STATEMENT_FORMAT + 1);
        let reason = drift(&old_format, &facts, 7).expect("a newer format drifts");
        assert!(reason.contains("format"), "{reason}");
        let mut no_format = good.clone();
        no_format.statement_format = None;
        let reason = drift(&no_format, &facts, 7).expect("a missing format drifts");
        assert!(reason.contains("format"), "{reason}");

        // A changed retention: the rendered text differs, the
        // fingerprint still matches.
        let reason = drift(&good, &facts, 30).expect("a retention change drifts");
        assert!(reason.contains("differs from the recorded one"), "{reason}");

        // A fingerprint mismatch without a text change is impossible
        // here, so the record keeps the old fingerprint while the facts
        // moved (the reverse order of the swap test).
        let mut moved = facts.clone();
        moved.nodes.push(node("check", "script"));
        let reason = drift(&good, &moved, 7).expect("changed facts drift");
        assert!(
            reason.contains("fingerprint"),
            "the refusal names the fingerprint: {reason}"
        );

        // An S2-shaped record with no new fields at all.
        let mut s2 = good.clone();
        s2.tier = None;
        s2.fingerprint = None;
        s2.statement = None;
        s2.statement_sha256 = None;
        s2.statement_format = None;
        let reason = drift(&s2, &facts, 7).expect("an old record drifts");
        assert!(
            reason.contains("`nearbee list`"),
            "the operator is told to relist: {reason}"
        );
    }
}
