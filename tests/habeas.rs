// {test}`requirements.integration.self_test`
#[test]
fn habeas() {
    let mut cargo = std::process::Command::new(env!("CARGO"));
    // Cargo exports this package's metadata to tests. Inheriting it in a new
    // build invalidates dependencies such as ring that track those variables.
    cargo.env_remove("CARGO_MANIFEST_DIR");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("CARGO_PKG_") {
            cargo.env_remove(name);
        }
    }
    let status = cargo
        .args([
            "run",
            "--locked",
            "--release",
            "-p",
            "habeas",
            "--",
            "validate",
        ])
        .status()
        .expect("could not launch release-mode repository validation");
    assert!(status.success(), "repository validation failed ({status})");
}
