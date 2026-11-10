//! The runner ignores the system proxy settings: with `HTTP_PROXY`,
//! `ALL_PROXY` (and their lower-case forms) pointing at another server,
//! every request still goes straight to apb. Proxy variables are process
//! wide and the workspace forbids `unsafe` (`set_var`), so the test runs
//! itself again as a child process with the variables set.

mod common;

use std::path::Path;
use std::process::Command;

use common::*;
use nearbee_apb::ApbRunner;
use nearbee_core::runner::{RunId, Runner};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHILD: &str = "NEARBEE_PROXY_TEST_APB_URL";
const TEST: &str = "system_proxy_settings_are_ignored";

#[tokio::test(flavor = "multi_thread")]
async fn system_proxy_settings_are_ignored() {
    if let Ok(url) = std::env::var(CHILD) {
        // Child: connect and read one run through apb at `url`.
        let mut o = opts_for(&url);
        o.timing.read_retry_delays.clear();
        let runner = ApbRunner::connect(o, Path::new(WS_PATH)).await.unwrap();
        runner
            .run(&RunId::new("smoke-own-1794253399097"))
            .await
            .unwrap();
        return;
    }
    let apb = MockServer::start().await;
    recorded(&fixture("projects"), true)
        .expect(1)
        .mount(&apb)
        .await;
    recorded(&fixture("run_succeeded"), true)
        .expect(1)
        .mount(&apb)
        .await;
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(502))
        .mount(&proxy)
        .await;
    let exe = std::env::current_exe().unwrap();
    let uri = apb.uri();
    let proxy_uri = proxy.uri();
    let out = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(exe);
        cmd.args([TEST, "--exact", "--nocapture"]).env(CHILD, uri);
        for v in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            cmd.env(v, &proxy_uri);
        }
        for v in ["NO_PROXY", "no_proxy"] {
            cmd.env_remove(v);
        }
        cmd.output().unwrap()
    })
    .await
    .unwrap();
    assert!(
        out.status.success(),
        "child failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        proxy.received_requests().await.unwrap().is_empty(),
        "a request went through the proxy"
    );
}

fn opts_for(url: &str) -> nearbee_apb::ApbOptions {
    nearbee_apb::ApbOptions {
        url: url.to_owned(),
        api_key: None,
        redact: vec![],
        timing: fast_timing(),
        bin: std::path::PathBuf::from("apb"),
        config_dir: std::path::PathBuf::from("/tmp/nearbee-test-apb-config"),
    }
}
