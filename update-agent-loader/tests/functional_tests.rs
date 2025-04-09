use tokio::runtime::Runtime;
use wiremock::{
    matchers::{method, path as path_match},
    Mock, MockServer, ResponseTemplate,
};

#[test]
fn test_cli_args_parsing() {
    let result = escargot::CargoBuild::new()
        .bin("update-agent-loader")
        .current_release()
        .current_target()
        .run()
        .unwrap()
        .command()
        .args(["--help"])
        .output()
        .unwrap();

    assert!(result.status.success());
    let stdout = String::from_utf8(result.stdout).expect("stdout is UTF-8 string");
    assert!(stdout.contains("URL to download the executable from"));
    assert!(stdout.contains("Arguments to pass to the executable"));
}

#[test]
fn test_download_and_execute_http() {
    // Start a mock HTTP server
    let rt = Runtime::new().unwrap();
    let mock_server = rt.block_on(MockServer::start());

    // Create the mock endpoint for our "executable"
    rt.block_on(
        Mock::given(method("GET"))
            .and(path_match("/binary"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    "mock executable content",
                    "application/octet-stream",
                ),
            )
            .mount(&mock_server),
    );

    // Build the URL to our mock server
    let url = format!("{}/binary", mock_server.uri());

    let result = escargot::CargoBuild::new()
        .bin("update-agent-loader")
        .current_release()
        .current_target()
        .run()
        .unwrap()
        .command()
        .args(["--url", &url, "--help"])
        .output()
        .unwrap();

    assert!(result.status.success());
    let stdout = String::from_utf8(result.stdout).expect("stdout is UTF-8 string");
    assert!(stdout.contains("update-agent-loader"));
    assert!(stdout.contains("OPTIONS"));
}
