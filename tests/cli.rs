use std::process::Command;

// {test}`requirements.cli.invocation`
#[test]
fn renderer_support_and_invalid_commands() {
    let binary = env!("CARGO_BIN_EXE_habeas");
    let supported = Command::new(binary)
        .args(["mdbook", "supports", "html"])
        .output()
        .unwrap();
    assert!(supported.status.success());

    let unsupported = Command::new(binary)
        .args(["mdbook", "supports", "epub"])
        .output()
        .unwrap();
    assert!(!unsupported.status.success());

    for args in [["unknown"].as_slice(), ["setup", "--wrong"].as_slice()] {
        let output = Command::new(binary).args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: habeas"));
    }
}
