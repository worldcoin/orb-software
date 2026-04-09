use color_eyre::{eyre::WrapErr as _, Result};
use orb_se050_reprovision::cli::{CliOutput, MockChild};

use crate::harness::CliProxy;

mod harness;

#[tokio::test]
async fn foobar() -> Result<()> {
    let _ = color_eyre::install();
    let _ = orb_telemetry::TelemetryConfig::new().init();
    let mock_server = wiremock::MockServer::start().await;
    // TODO: Add mocked endpoints

    let harness = harness::Harness::builder()
        .seed(1337)
        .mocked_server(mock_server)
        .build();
    let cfg = harness.make_program_cfg();

    let cli_io_fut = async {
        let CliProxy { stdin, stdout, .. } = harness.mocked_cli;
        let input = stdin
            .recv_async()
            .await
            .expect("expected at least one bytes");
        assert!(
            stdin.recv_async().await.is_err(),
            "expected no more than one bytes"
        );
        let output = handle_io(&input);
        stdout
            .send_async(output.into())
            .await
            .expect("expected program to still be listening to stdout");
        assert_eq!(stdout.sender_count(), 1);
        drop(stdout);
    };
    let run_fut = async move {
        orb_se050_reprovision::run(cfg)
            .await
            .expect("error in program")
    };
    let ((), ()) = tokio::join!(run_fut, cli_io_fut);

    Ok(())
}

fn handle_io(_stdin: &[u8]) -> Vec<u8> {
    // TODO: make it not stubbed
    vec![0; 10]
}
