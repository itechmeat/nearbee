//! CLI tests of `nearbee agent add` against a wiremock market.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use nearbee::{Env, agent_add_from};
use nearbee_market_fixture as fx;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "aat_0123456789abcdef0123456789abcdef";
const HANDLE: &str = "repo-audit-fast";
/// Real market agent ids are UUIDs (the live `repo-audit-fast` record).
const AGENT_ID: &str = "01980a2e-7c1b-7000-8000-000000000003";

fn env_of(server: &MockServer, home: &Path) -> Env<'static> {
    let map: HashMap<String, String> = HashMap::from([
        ("MARKET_URL".to_string(), server.uri()),
        (
            "NEARBEE_CREDENTIALS".to_string(),
            home.join("nearbee")
                .join("credentials")
                .display()
                .to_string(),
        ),
        ("HOME".to_string(), home.display().to_string()),
    ]);
    // One leaked map per test process; the tests are small and the counts
    // are fixed, so this stays bounded.
    let var: &'static dyn Fn(&str) -> Option<String> =
        Box::leak(Box::new(move |k: &str| map.get(k).cloned()));
    Env {
        cwd: Path::new("/"),
        var,
        timing: Default::default(),
    }
}

async fn mounts(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{HANDLE}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fx::agent_public(AGENT_ID, HANDLE, "draft")),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![])))
        .mount(server)
        .await;
}

fn output(out: &[u8]) -> String {
    String::from_utf8_lossy(out).to_string()
}

#[tokio::test]
async fn an_unknown_handle_without_an_id_fails_and_tells_the_operator() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    Mock::given(method("GET"))
        .and(path("/v1/agents/ghost"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(fx::refusal("not_found", "agent not found")),
        )
        .mount(&server)
        .await;
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        "ghost",
        None,
        &env,
        &mut out,
        &mut Cursor::new(TOKEN.as_bytes()),
    )
    .await;
    assert_eq!(code, 1);
    let text = output(&out);
    // A draft agent is invisible to the lookup: the operator is told where
    // the id comes from.
    assert!(text.contains("not publicly visible"), "{text}");
    assert!(text.contains("--agent-id"), "{text}");
    assert!(text.contains("settings page"), "{text}");
    assert!(!home.path().join("nearbee").join("credentials").exists());
}

#[tokio::test]
async fn a_404_with_an_operator_id_stores_the_given_id_and_says_so() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    let id = "01980a2e-7c1b-7000-8000-000000000001";
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{HANDLE}")))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(fx::refusal("not_found", "agent not found")),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fx::assignments(vec![])))
        .mount(&server)
        .await;
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        Some(id.to_string()),
        &env,
        &mut out,
        &mut Cursor::new(TOKEN.as_bytes()),
    )
    .await;
    assert_eq!(code, 0, "{}", output(&out));
    let text = output(&out);
    assert!(text.contains("the operator"), "{text}");
    assert!(text.contains("--agent-id"), "{text}");
    let store =
        nearbee_core::credentials::Store::at(home.path().join("nearbee").join("credentials"));
    let creds = store.get(HANDLE).unwrap().unwrap();
    assert_eq!(creds.agent_id, id);
}

#[tokio::test]
async fn an_operator_id_that_is_not_a_uuid_is_refused_without_a_request() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        Some("ag_not_a_uuid".to_string()),
        &env,
        &mut out,
        &mut Cursor::new(TOKEN.as_bytes()),
    )
    .await;
    assert_eq!(code, 1);
    assert!(output(&out).contains("--agent-id must be the agent id"));
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(!home.path().join("nearbee").join("credentials").exists());
}

#[tokio::test]
async fn a_lookup_answer_with_a_different_operator_id_is_refused() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mounts(&server).await;
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        Some("01980a2e-7c1b-7000-8000-000000000002".to_string()),
        &env,
        &mut out,
        &mut Cursor::new(TOKEN.as_bytes()),
    )
    .await;
    assert_eq!(code, 1);
    let text = output(&out);
    assert!(text.contains("differs from the agent id"), "{text}");
    assert!(text.contains(AGENT_ID), "{text}");
    assert!(!home.path().join("nearbee").join("credentials").exists());
}

#[tokio::test]
async fn an_operator_id_matching_the_record_is_accepted() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mounts(&server).await;
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        Some(AGENT_ID.to_string()),
        &env,
        &mut out,
        &mut Cursor::new(TOKEN.as_bytes()),
    )
    .await;
    assert_eq!(code, 0, "{}", output(&out));
    let text = output(&out);
    assert!(text.contains("the market record"), "{text}");
    assert!(!text.contains("the operator"), "{text}");
}

#[tokio::test]
async fn a_refused_token_fails_and_stores_nothing() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    // The public record answers; the authenticated probe answers 401: the
    // token does not belong to this agent.
    Mock::given(method("GET"))
        .and(path(format!("/v1/agents/{HANDLE}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fx::agent_public(AGENT_ID, HANDLE, "draft")),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agents/me/assignments"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(fx::refusal("unauthorized", "bad token")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        None,
        &env,
        &mut out,
        &mut Cursor::new(TOKEN.as_bytes()),
    )
    .await;
    assert_eq!(code, 1);
    let text = output(&out);
    assert!(text.contains("the token was refused"), "{text}");
    assert!(!text.contains(TOKEN), "{text}");
    assert!(!home.path().join("nearbee").join("credentials").exists());
}

#[tokio::test]
async fn a_good_token_is_stored_and_printed_masked() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mounts(&server).await;
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        None,
        &env,
        &mut out,
        &mut Cursor::new(format!("{TOKEN}\n").as_bytes()),
    )
    .await;
    assert_eq!(code, 0, "{}", output(&out));
    let text = output(&out);
    assert!(text.contains(HANDLE) && text.contains(AGENT_ID), "{text}");
    assert!(text.contains("aat_...cdef"), "{text}");
    assert!(!text.contains(TOKEN), "{text}");
    // The store holds the pair, with mode 0600.
    let store =
        nearbee_core::credentials::Store::at(home.path().join("nearbee").join("credentials"));
    let creds = store.get(HANDLE).unwrap().unwrap();
    assert_eq!(creds.agent_id, AGENT_ID);
    assert_eq!(creds.token.expose(), TOKEN);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[tokio::test]
async fn a_malformed_token_is_refused_before_any_request() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&server, home.path());
    let mut out = Vec::new();
    let code = agent_add_from(
        None,
        HANDLE,
        None,
        &env,
        &mut out,
        &mut Cursor::new(b"sk-not-a-market-token".as_slice()),
    )
    .await;
    assert_eq!(code, 1);
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(output(&out).contains("does not look like an agent token"));
}
