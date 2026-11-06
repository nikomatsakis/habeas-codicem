use std::process::Command;

// {test}`requirements.cli.invocation`
#[test]
fn renderer_support_and_invalid_setup_option() {
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

    for args in [
        ["agents"].as_slice(),
        ["setup", "--wrong"].as_slice(),
        ["setup", "--install-skills"].as_slice(),
        ["setup", "--path", ".", "--install-skills"].as_slice(),
    ] {
        let output = Command::new(binary).args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: habeas"));
    }
}

// {test}`requirements.cli.invocation`
// {test}`reference.cli.unknown_command`
#[test]
fn unknown_command_fails_with_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("unknown")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: habeas"));
}

// {test}`requirements.cli.clap`
#[test]
fn clap_defines_commands_options_and_usage_errors() {
    let binary = env!("CARGO_BIN_EXE_habeas");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.starts_with("Usage: habeas <COMMAND>"), "{help}");
    for command in [
        "mdbook",
        "setup",
        "status",
        "info",
        "author",
        "add-input",
        "help",
    ] {
        assert!(
            help.contains(&format!("\n  {command} ")),
            "{command}: {help}"
        );
    }
    assert!(
        !help.contains("hook"),
        "hidden commands stay out of help: {help}"
    );

    let status = Command::new(binary)
        .args(["status", "--help"])
        .output()
        .unwrap();
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(
        status.contains("--all") && status.contains("--verbose"),
        "{status}"
    );

    // The alias parses to the same command.
    let alias = Command::new(binary)
        .args(["author-evidence", "--help"])
        .output()
        .unwrap();
    assert!(alias.status.success());
    assert!(String::from_utf8_lossy(&alias.stdout).contains("Usage: habeas author"));

    for (args, expected) in [
        (
            ["status", "--bogus"].as_slice(),
            "unexpected argument '--bogus'",
        ),
        (["info"].as_slice(), "required arguments were not provided"),
        (["author", "x", "--all"].as_slice(), "cannot be used with"),
    ] {
        let output = Command::new(binary).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected) && stderr.contains("Usage: habeas"),
            "{args:?}: {stderr}"
        );
    }
}
