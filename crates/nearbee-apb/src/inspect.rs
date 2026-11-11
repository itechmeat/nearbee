//! The playbook inspection: builds the execution facts (design 2) from
//! apb's playbook detail route, the profile detail routes, the
//! workspace's `opencode.json` and `.apb/config.yaml`, and the instance's
//! environment and version. Every fact the configuration does not give
//! (an absent key, a 404, a file that does not parse) is reported unknown
//! with the reason, never guessed. A failed read of the instance itself (a
//! transport error, a 5xx, a failed `systemctl`) is an error of the whole
//! inspection instead: it says nothing about the configuration, and the
//! serving loop holds the listing for that round rather than declining.

use nearbee_core::runner::{PlaybookId, RunnerError};
use nearbee_core::tiers::{
    ExecutionFacts, ExecutorFact, Known, NodeFact, ProfileFact, ProfileRef, ProviderRoute,
};
use yaml_serde::Value as Yaml;

use crate::{ApbRunner, Kind, path_segment, str_field};

/// Reads a playbook reference as apb writes it: a string (scope `auto`)
/// or `{name, scope}` (apb `crates/apb-core/src/profile.rs`).
fn profile_ref(v: Option<&Yaml>) -> Option<ProfileRef> {
    match v? {
        Yaml::String(name) => Some(ProfileRef {
            name: name.clone(),
            scope: "auto".into(),
        }),
        Yaml::Mapping(m) => {
            let name = m.get("name").and_then(|v| v.as_str()).map(str::to_owned)?;
            let scope = m.get("scope").and_then(|v| v.as_str()).unwrap_or("auto");
            Some(ProfileRef {
                name,
                scope: scope.to_owned(),
            })
        }
        _ => None,
    }
}

fn string_list(v: Option<&Yaml>) -> Vec<String> {
    let mut out: Vec<String> = v
        .and_then(|v| v.as_sequence())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Yaml::String(s) => Some(s.clone()),
                    // A connector binding or a skill reference may be
                    // written as an object; the name is what the facts
                    // need.
                    Yaml::Mapping(m) => ["connector", "name", "id"]
                        .iter()
                        .find_map(|k| m.get(*k).and_then(|v| v.as_str()))
                        .map(str::to_owned),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// Parses the profile's YAML (the detail route's `profile_yaml`) into the
/// facts the check and the statement need. `None` on a document that does
/// not parse, and on a malformed fallback or tier entry: a routing entry
/// the facts cannot read makes the whole profile unknown, it is never
/// dropped in silence. The caller reports the reference unknown.
pub(crate) fn profile_fact(reference: ProfileRef, yaml: &str) -> Option<ProfileFact> {
    let root: Yaml = yaml_serde::from_str(yaml).ok()?;
    let executor = |v: Option<&Yaml>| -> Option<ExecutorFact> {
        let v = v?;
        Some(ExecutorFact {
            agent: v.get("agent")?.as_str()?.to_owned(),
            model: v.get("model")?.as_str()?.to_owned(),
        })
    };
    let ex = executor(root.get("executor"))?;
    // apb keeps fallbacks under `executor` (the shipped profiles and the
    // recorded profile detail); a top-level list is read as well.
    let fallbacks = match root
        .get("executor")
        .and_then(|e| e.get("fallbacks"))
        .or_else(|| root.get("fallbacks"))
    {
        None | Some(Yaml::Null) => Vec::new(),
        Some(list) => list
            .as_sequence()?
            .iter()
            .map(|item| executor(Some(item)))
            .collect::<Option<Vec<_>>>()?,
    };
    // Tiers are a YAML map of name to `{agent, model, for}` (apb's
    // `ProfileTiers`), declaration order kept.
    let mut tiers = Vec::new();
    match root.get("tiers") {
        None | Some(Yaml::Null) => {}
        Some(t) => {
            for (k, v) in t.as_mapping()? {
                tiers.push((k.as_str()?.to_owned(), executor(Some(v))?));
            }
        }
    }
    let env_policy = root
        .get("process_env")
        .and_then(|p| p.get("policy"))
        .and_then(|p| p.as_str())
        .unwrap_or("inherit")
        .to_owned();
    let env_allow = string_list(root.get("process_env").and_then(|p| p.get("allow")));
    Some(ProfileFact {
        reference: reference.clone(),
        executor: ex,
        fallbacks,
        tiers,
        environment: root
            .get("environment")
            .and_then(|v| v.as_str())
            // Absent is apb's default, not `minimal`: the attested check
            // then refuses it (no stronger claim than the file makes).
            .unwrap_or("unset")
            .to_owned(),
        skills: string_list(root.get("skills")),
        env_policy,
        env_allow,
    })
}

/// The playbook structure from its YAML: nodes with their profile
/// references, connectors and sub-playbook bindings, and the default
/// profile. Fails when the source does not parse.
pub(crate) fn playbook_structure(
    source: &str,
) -> Result<(String, Option<ProfileRef>, Vec<NodeFact>), RunnerError> {
    let root: Yaml = yaml_serde::from_str(source).map_err(|e| RunnerError::Protocol {
        detail: format!("playbook YAML does not parse: {e}"),
    })?;
    let playbook_id = root
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RunnerError::Protocol {
            detail: "playbook YAML has no id".into(),
        })?
        .to_owned();
    let default_profile = profile_ref(root.get("defaults").and_then(|d| d.get("profile")));
    let mut nodes = Vec::new();
    if let Some(items) = root.get("nodes").and_then(|n| n.as_sequence()) {
        for item in items {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RunnerError::Protocol {
                    detail: "playbook node without id".into(),
                })?
                .to_owned();
            // `interactive` is only meaningful on an agent_task node and
            // defaults to false (apb schema 2).
            let interactive = item
                .get("interactive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // A sub-playbook binding: `playbook: id` or `{id, scope}`.
            let playbook_ref = match item.get("playbook") {
                Some(Yaml::String(id)) => Some(id.clone()),
                Some(Yaml::Mapping(m)) => m.get("id").and_then(|v| v.as_str()).map(str::to_owned),
                _ => None,
            };
            nodes.push(NodeFact {
                id,
                kind: item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                interactive,
                profile: profile_ref(item.get("profile")),
                connectors: string_list(item.get("connectors")),
                playbook_ref,
            });
        }
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((playbook_id, default_profile, nodes))
}

/// OpenCode's system configuration directory on Linux (OpenCode 2.2.5).
const OPENCODE_SYSTEM_DIR: &str = "/etc/opencode";

/// What OpenCode's own installer leaves in `~/.opencode` (the binary and
/// its plugin dependencies); any other entry in a `.opencode/` above the
/// workspace is configuration OpenCode would merge.
const OPENCODE_INSTALL_FILES: [&str; 6] = [
    "bin",
    "node_modules",
    "package.json",
    "package-lock.json",
    "bun.lock",
    ".gitignore",
];

/// The keys a model entry under `provider.<name>.models` may carry
/// without changing what is called: display and metadata only. Any other
/// key (`id`, `options`, `provider`, `headers` and the rest) aliases or
/// overrides the model.
const MODEL_METADATA_KEYS: [&str; 9] = [
    "name",
    "limit",
    "cost",
    "attachment",
    "reasoning",
    "temperature",
    "tool_call",
    "modalities",
    "release_date",
];

/// The `NAME=value` items of `systemctl show -p Environment` output
/// (`Environment=A=1 "B=two words"`), unquoted.
fn env_items(show: &str) -> Vec<String> {
    let body = show
        .trim()
        .strip_prefix("Environment=")
        .unwrap_or(show.trim());
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    items.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        items.push(cur);
    }
    items
}

/// The value of `name` in `systemctl show -p Environment` output
/// (`Environment=A=1 "B=two words"`), unquoted; `None` when unset.
pub(crate) fn env_value(show: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    env_items(show)
        .into_iter()
        .rev()
        .find_map(|item| item.strip_prefix(&prefix).map(str::to_owned))
}

/// The first variable of the instance environment that could redirect a
/// Nearbee command's direct model call (spec `privacy-tiers`, Direct call
/// route): `NEARAI_CLOUD_URL`, any `NEARBEE_CLOUD_` variable or a proxy
/// variable, in any case.
pub(crate) fn redirecting_variable(show: &str) -> Option<String> {
    env_items(show).into_iter().find_map(|item| {
        let name = item.split('=').next().unwrap_or_default().to_owned();
        let upper = name.to_ascii_uppercase();
        let hit = upper == "NEARAI_CLOUD_URL"
            || upper.starts_with("NEARBEE_CLOUD_")
            || ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"].contains(&upper.as_str());
        hit.then_some(name)
    })
}

/// The provider routes of an `opencode.json` text, sorted by name; `Err`
/// with the reason when it does not parse or has no provider object.
pub(crate) fn parse_routes(text: &str) -> Result<Vec<ProviderRoute>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let Some(providers) = v.get("provider").and_then(|p| p.as_object()) else {
        return Err("no provider object".into());
    };
    let mut routes: Vec<ProviderRoute> = providers
        .iter()
        .map(|(name, p)| {
            // A provider without `options` is kept with empty fields: the
            // tier check names it, silence would hide it.
            let options = p.get("options");
            let mut model_overrides: Vec<String> = Vec::new();
            match p.get("models") {
                None | Some(serde_json::Value::Null) => {}
                Some(serde_json::Value::Object(models)) => {
                    for (id, entry) in models {
                        let keys: Vec<&str> = match entry.as_object() {
                            Some(o) => o
                                .keys()
                                .map(String::as_str)
                                .filter(|k| !MODEL_METADATA_KEYS.contains(k))
                                .collect(),
                            None => vec!["(not an object)"],
                        };
                        if !keys.is_empty() {
                            model_overrides.push(format!("{id}: {}", keys.join(", ")));
                        }
                    }
                }
                Some(_) => model_overrides.push("models is not an object".into()),
            }
            model_overrides.sort();
            ProviderRoute {
                name: name.clone(),
                base_url: options
                    .and_then(|o| o.get("baseURL"))
                    .and_then(|b| b.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                api_key_ref: options
                    .and_then(|o| o.get("apiKey"))
                    .and_then(|k| k.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                model_overrides,
            }
        })
        .collect();
    routes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(routes)
}

impl ApbRunner {
    /// Fetches one named profile in one scope. `Ok(Ok(None))` is a 404:
    /// the scope does not hold the profile. `Ok(Err(reason))` is a
    /// configuration fact the answer does not give (a name apb cannot hold,
    /// no `profile_yaml`, a document that does not parse), the unknown
    /// reason (design 2.2). `Err` is a failed read (a transport error, a
    /// 5xx), which fails the whole inspection.
    pub(crate) async fn fetch_profile(
        &self,
        name: &str,
        scope: &str,
    ) -> Result<Result<Option<ProfileFact>, String>, RunnerError> {
        let name = match path_segment(name) {
            Ok(n) => n,
            Err(e) => return Ok(Err(e.to_string())),
        };
        let path = format!("/api/profiles/{name}");
        let query = [("workspace", self.workspace.as_str()), ("scope", scope)];
        let v = match self.client.get(&path, &query, Kind::Read).await {
            Ok(v) => v,
            Err(RunnerError::NotFound { .. }) => return Ok(Ok(None)),
            Err(e) => return Err(e),
        };
        let Some(yaml) = str_field(&v, "profile_yaml") else {
            return Ok(Err(format!("GET {path}: the answer has no profile_yaml")));
        };
        let reference = ProfileRef {
            name: name.to_owned(),
            scope: scope.to_owned(),
        };
        Ok(profile_fact(reference, &yaml)
            .map(Some)
            .ok_or_else(|| format!("GET {path}: profile_yaml does not parse")))
    }

    /// Resolves one reference: `project` and `global` read only their
    /// scope, `auto` reads the project scope and falls back to the global
    /// one on 404 (design 2.2). The fact stays keyed by the reference as
    /// the playbook names it.
    pub(crate) async fn resolve_profile(
        &self,
        reference: &ProfileRef,
    ) -> Result<(ProfileRef, Known<ProfileFact>), RunnerError> {
        let scopes: Vec<&str> = match reference.scope.as_str() {
            "project" | "global" => vec![reference.scope.as_str()],
            _ => vec!["project", "global"],
        };
        let mut last = Known::Unknown("the scope is not project, global or auto".to_owned());
        for scope in scopes {
            match self.fetch_profile(&reference.name, scope).await? {
                Ok(Some(fact)) => return Ok((reference.clone(), Known::Yes(fact))),
                Ok(None) => {
                    last = Known::Unknown(format!(
                        "GET /api/profiles/{}?scope={}: HTTP 404",
                        reference.name, scope
                    ));
                }
                Err(reason) => return Ok((reference.clone(), Known::Unknown(reason))),
            }
        }
        Ok((reference.clone(), last))
    }

    /// Reads the instance environment the engine hands to agent processes,
    /// via `systemctl --user show nearbee-apb -p Environment`. A failed
    /// read (the program cannot run, a non-zero exit, a signal) fails the
    /// inspection: it says nothing about the configuration.
    pub(crate) async fn instance_env(&self) -> Result<String, RunnerError> {
        let bin = self.systemctl_bin.clone();
        let workspace = self.workspace_path.clone();
        let config_dir = self.cli_config_dir.clone();
        tokio::task::spawn_blocking(move || {
            let borrowed = ["--user", "show", "nearbee-apb", "-p", "Environment"];
            crate::cli::run(
                &bin,
                &borrowed,
                &workspace,
                &config_dir,
                &[],
                std::time::Duration::from_secs(10),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("the instance environment task failed: {e}"))
        .and_then(|r| r)
        .and_then(|out| {
            if out.status == Some(0) {
                Ok(out.stdout)
            } else {
                Err(format!(
                    "systemctl exited with {}: {}",
                    out.status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "a signal".into()),
                    out.stderr.trim()
                ))
            }
        })
        .map_err(|detail| RunnerError::Protocol {
            detail: format!("the instance environment cannot be read: {detail}"),
        })
    }

    /// The provider routes from `<workspace>/opencode.json`
    /// (`provider.<name>.options.baseURL`, `options.apiKey` and the model
    /// entries that alias or override a model). Unknown when the file is
    /// missing or unreadable, when the instance environment carries any
    /// `OPENCODE_` variable (design 2.3) or a variable that could redirect
    /// a direct model call (`NEARAI_CLOUD_URL`, `NEARBEE_CLOUD_*`, a proxy
    /// variable; change `doc-digest-and-paid-demo`, design 5.2), or when
    /// OpenCode would merge
    /// another configuration over the workspace file (OpenCode 2.2.5's
    /// config discovery, read at the installed release): the instance's
    /// `XDG_CONFIG_HOME` is not Nearbee's own directory or that directory
    /// holds an `opencode` entry; the workspace has a `.opencode/`
    /// directory; the system directory `/etc/opencode` exists; or a
    /// directory above the workspace holds `opencode.json`,
    /// `opencode.jsonc` or a `.opencode/` with more than OpenCode's own
    /// install files. A failed environment read fails the inspection.
    pub(crate) async fn routes(&self) -> Result<Known<Vec<ProviderRoute>>, RunnerError> {
        let env = self.instance_env().await?;
        if env.contains("OPENCODE_") {
            return Ok(Known::Unknown(
                "the apb instance environment sets an OPENCODE_ variable".into(),
            ));
        }
        if let Some(name) = redirecting_variable(&env) {
            return Ok(Known::Unknown(format!(
                "the apb instance environment sets {name}, which could redirect a direct model call"
            )));
        }
        if let Some(reason) = self.foreign_opencode_config(&env) {
            return Ok(Known::Unknown(reason));
        }
        let path = self.workspace_path.join("opencode.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return Ok(Known::Unknown(format!("{}: {e}", path.display())));
            }
        };
        Ok(match parse_routes(&text) {
            Ok(routes) => Known::Yes(routes),
            Err(e) => Known::Unknown(format!("{}: {e}", path.display())),
        })
    }

    /// Why OpenCode would read a configuration other than the workspace's
    /// `opencode.json`, or `None` (see [`ApbRunner::routes`]).
    fn foreign_opencode_config(&self, env: &str) -> Option<String> {
        let xdg = env_value(env, "XDG_CONFIG_HOME");
        if xdg.as_deref().map(std::path::Path::new) != Some(self.xdg_config_home.as_path()) {
            return Some(format!(
                "the apb instance's XDG_CONFIG_HOME is {}, not Nearbee's own directory {}",
                xdg.as_deref().unwrap_or("not set"),
                self.xdg_config_home.display()
            ));
        }
        if std::fs::symlink_metadata(self.xdg_config_home.join("opencode")).is_ok() {
            return Some(format!(
                "{} holds an opencode configuration",
                self.xdg_config_home.display()
            ));
        }
        if std::fs::symlink_metadata(self.workspace_path.join(".opencode")).is_ok() {
            return Some("the workspace has a .opencode directory".into());
        }
        if std::fs::symlink_metadata(OPENCODE_SYSTEM_DIR).is_ok() {
            return Some(format!("the system directory {OPENCODE_SYSTEM_DIR} exists"));
        }
        for dir in self.workspace_path.ancestors().skip(1) {
            for name in ["opencode.json", "opencode.jsonc"] {
                if std::fs::symlink_metadata(dir.join(name)).is_ok() {
                    return Some(format!(
                        "{} above the workspace would be merged",
                        dir.join(name).display()
                    ));
                }
            }
            let dot = dir.join(".opencode");
            match std::fs::read_dir(&dot) {
                Ok(entries) => {
                    for entry in entries {
                        let Ok(entry) = entry else {
                            return Some(format!("{} cannot be listed", dot.display()));
                        };
                        let name = entry.file_name();
                        if !OPENCODE_INSTALL_FILES.contains(&name.to_string_lossy().as_ref()) {
                            return Some(format!(
                                "{} above the workspace holds {}",
                                dot.display(),
                                name.to_string_lossy()
                            ));
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => {}
                Err(e) => return Some(format!("{}: {e}", dot.display())),
            }
        }
        None
    }

    /// The decisions setting from `<workspace>/.apb/config.yaml`
    /// (design 2.4): `false` and `true` are known, absent unknown.
    pub(crate) fn decisions(&self) -> Known<bool> {
        let path = self.workspace_path.join(".apb").join("config.yaml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return Known::Unknown(format!("{}: {e}", path.display())),
        };
        let root: Yaml = match yaml_serde::from_str(&text) {
            Ok(r) => r,
            Err(e) => return Known::Unknown(format!("{}: {e}", path.display())),
        };
        match root
            .get("decisions")
            .and_then(|d| d.get("enabled"))
            .and_then(|e| e.as_bool())
        {
            Some(b) => Known::Yes(b),
            None => Known::Unknown(format!("{}: decisions.enabled is not set", path.display())),
        }
    }

    /// Whether apb compacts context: only the recorded beta.6 release can
    /// not (design 2.5); any other version is unknown. A failed health
    /// read fails the inspection.
    pub(crate) async fn compaction(&self) -> Result<Known<bool>, RunnerError> {
        let v = self.client.get("/api/health", &[], Kind::Read).await?;
        Ok(match str_field(&v, "version") {
            Some(version) if version == "1.0.0-beta.6" => Known::Yes(false),
            Some(version) => Known::Unknown(format!("apb version {version}")),
            None => Known::Unknown("the health answer has no version".into()),
        })
    }

    /// The facts for a playbook YAML plus everything around it.
    pub(crate) async fn inspect_common(
        &self,
        source: Option<&str>,
        id: &PlaybookId,
    ) -> Result<ExecutionFacts, RunnerError> {
        let source = match source {
            Some(s) => s.to_owned(),
            None => {
                let path = format!("/api/playbooks/{}", path_segment(id.as_str())?);
                let v = self.client.get(&path, &self.ws(), Kind::Read).await?;
                str_field(&v, "yaml").ok_or_else(|| {
                    self.client
                        .protocol(&format!("GET {path}: the answer has no yaml"))
                })?
            }
        };
        let (playbook_id, default_profile, nodes) = playbook_structure(&source)?;
        // Every distinct reference from the default profile and the nodes,
        // sorted (design 2).
        let mut refs: Vec<ProfileRef> = default_profile.clone().into_iter().collect();
        for node in &nodes {
            if let Some(profile) = &node.profile {
                refs.push(profile.clone());
            }
        }
        refs.sort();
        refs.dedup();
        let mut profiles = Vec::new();
        for reference in &refs {
            profiles.push(self.resolve_profile(reference).await?);
        }
        profiles.sort_by(|a, b| {
            (a.0.name.clone(), a.0.scope.clone()).cmp(&(b.0.name.clone(), b.0.scope.clone()))
        });
        Ok(ExecutionFacts {
            playbook_id,
            default_profile,
            nodes,
            profiles,
            routes: self.routes().await?,
            decisions_enabled: self.decisions(),
            compaction_enabled: self.compaction().await?,
            direct_models: Vec::new(),
            signed_models: Vec::new(),
        })
    }
}
