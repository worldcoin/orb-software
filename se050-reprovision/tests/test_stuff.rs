use color_eyre::{eyre::WrapErr as _, Result};
use orb_se050_reprovision::cli::{CliOutput, KeyInfo, Nonce};

use crate::harness::CliProxy;

mod harness;

#[tokio::test]
async fn stubbed_ca_works() -> Result<()> {
    let _ = color_eyre::install();
    let _ = orb_telemetry::TelemetryConfig::new().init();
    let mock_server = wiremock::MockServer::start().await;
    // TODO: Add mocked endpoints

    let builder = harness::Harness::builder()
        .seed(1337)
        .mocked_server(mock_server);
    let (harness, program_cfg) = builder.build();

    let cli_io_fut = async {
        let CliProxy { stdin, stdout, .. } = harness.cli_proxy;
        let input = stdin
            .recv_async()
            .await
            .expect("expected at least one bytes");
        assert!(
            stdin.recv_async().await.is_err(),
            "expected no more than one bytes"
        );
        let output = handle_io(&input).expect("failed to handle io");
        stdout
            .send_async(output.into())
            .await
            .expect("expected program to still be listening to stdout");
        assert_eq!(stdout.sender_count(), 1);
        drop(stdout);
    };
    let run_fut = async move {
        orb_se050_reprovision::run(program_cfg)
            .await
            .expect("error in program")
    };
    let ((), ()) = tokio::join!(run_fut, cli_io_fut);

    Ok(())
}

fn handle_io(stdin: &[u8]) -> Result<Vec<u8>> {
    // TODO: This will need to not be so stubbed as we actually implement functionality
    let _nonce = Nonce::try_from(stdin).wrap_err("failed to deserialize as a nonce")?;

    let dummy_key_info = KeyInfo {
        key: String::new(),
        signature: Vec::new(),
        extra_data: Vec::new(),
    };
    let output = CliOutput {
        jetson_authkey: dummy_key_info.clone(),
        attestation_key: dummy_key_info.clone(),
        iris_code_key: dummy_key_info,
    };

    Ok(serde_json::to_vec(&output).expect("infallible"))
}
