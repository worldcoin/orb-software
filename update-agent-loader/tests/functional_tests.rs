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
    assert!(stdout.contains("Arguments to pass to the downloaded executable"));
}

#[test]
fn test_download_and_execute_http() {
    // Start a mock HTTP server
    let rt = Runtime::new().unwrap();
    let mock_server = rt.block_on(MockServer::start());

    // Read /bin/echo for our test executable
    let echo_binary = std::fs::read("/bin/echo").expect("Failed to read /bin/echo");

    // Create the mock endpoint serving the echo binary
    rt.block_on(
        Mock::given(method("GET"))
            .and(path_match("/binary"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(echo_binary, "application/octet-stream"),
            )
            .mount(&mock_server),
    );

    // Build the URL to our mock server
    let url = format!("{}/binary", mock_server.uri());

    let test_string = "test string";
    let result = escargot::CargoBuild::new()
        .bin("update-agent-loader")
        .current_release()
        .current_target()
        .run()
        .unwrap()
        .command()
        .args(["--url", &url, "--args", test_string])
        .output()
        .unwrap();

    assert!(result.status.success());
    let stdout = String::from_utf8(result.stdout).expect("stdout is UTF-8 string");
    // check that echo printed the expected output
    assert_eq!(stdout.trim(), test_string);
}
