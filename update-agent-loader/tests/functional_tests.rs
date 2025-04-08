use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_cli_args_parsing() {
    // Test that the binary accepts --url and --args parameters with --help
    // This tests the argument parsing without trying to execute anything
    let mut cmd = Command::cargo_bin("update-agent-loader").unwrap();

    let result = cmd
        .args([
            "--url",
            "https://example.com/binary",
            "--args",
            "arg1",
            "arg2",
            "--help",
        ])
        .assert();

    // The help output should contain our argument descriptions
    result
        .success()
        .stdout(predicate::str::contains(
            "URL to download the executable from",
        ))
        .stdout(predicate::str::contains(
            "Arguments to pass to the executable",
        ));
}

#[test]
fn test_cli_args_defaults() {
    // Test that the binary can be run without specifying arguments
    // Uses --help to make it exit cleanly without trying to download
    let mut cmd = Command::cargo_bin("update-agent-loader").unwrap();

    let result = cmd.arg("--help").assert();

    // Should succeed even without specifying arguments
    result.success();
}
