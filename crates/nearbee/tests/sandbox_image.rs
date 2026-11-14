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
