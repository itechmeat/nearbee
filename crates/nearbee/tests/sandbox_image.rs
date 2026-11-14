//! The sandbox image's package list (spec `apb-sandbox`): a static check of
//! `sandbox/Dockerfile`; the integrator checks the built image itself.

#[test]
fn the_image_installs_socat_for_the_claude_code_bash_sandbox() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox/Dockerfile");
    let text = std::fs::read_to_string(path).unwrap();
    let apt = text
        .split("apt-get install")
        .nth(1)
        .and_then(|rest| rest.split("&& rm -rf /var/lib/apt/lists").next())
        .expect("one apt-get install step");
    assert!(
        apt.split_whitespace().any(|w| w == "socat"),
        "socat is not in the apt-get install step: {apt}"
    );
}

#[test]
fn the_image_carries_pinned_playwright_chromium_on_a_shared_path() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox/Dockerfile");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(
        text.contains("ENV PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright"),
        "PLAYWRIGHT_BROWSERS_PATH is not set to the shared path"
    );
    let version = text
        .lines()
        .find_map(|l| l.strip_prefix("ARG PLAYWRIGHT_VERSION="))
        .expect("a pinned PLAYWRIGHT_VERSION");
    assert!(
        version.split('.').count() == 3 && version.split('.').all(|p| p.parse::<u32>().is_ok()),
        "PLAYWRIGHT_VERSION is not an exact version: {version}"
    );
    assert!(
        text.contains("install --with-deps chromium chromium-headless-shell"),
        "the Playwright step does not install chromium and its headless shell"
    );
}
