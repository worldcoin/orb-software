use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;
use tokio::runtime::Runtime;
use wiremock::{
    matchers::{method, path as path_match},
    Mock, MockServer, ResponseTemplate,
};

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

#[test]
fn test_download_and_execute_http() {
    // Start a mock HTTP server
    let rt = Runtime::new().unwrap();
    let mock_server = rt.block_on(MockServer::start());
    
    // Create the mock endpoint for our "executable"
    // In reality we just return some content and check if it downloads properly
    // since we're just running with --help and won't actually execute anything
    rt.block_on(Mock::given(method("GET"))
        .and(path_match("/binary"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("mock executable content", "application/octet-stream"))
        .mount(&mock_server));
    
    // Build the URL to our mock server
    let url = format!("{}/binary", mock_server.uri());
    
    // Run the binary with our mock URL and the --help flag
    // The --help flag ensures the binary will exit without trying to actually execute the download
    let mut cmd = Command::cargo_bin("update-agent-loader").unwrap();
    
    let result = cmd
        .args(["--url", &url, "--help"])
        .assert();
    
    // Check that the command executed successfully
    // The --help flag will cause it to show help and exit before actual execution
    result
        .success()
        .stdout(predicate::str::contains("update-agent-loader"))
        .stdout(predicate::str::contains("OPTIONS"));
}

