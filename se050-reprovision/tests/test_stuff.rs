use std::time::Duration;

use color_eyre::{eyre::WrapErr as _, Result};
use rand::SeedableRng;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    select,
};
use tracing::info;

mod harness;

#[tokio::test]
async fn foobar() -> Result<()> {
    let _ = color_eyre::install();
    let _ = orb_telemetry::TelemetryConfig::new().init();
    let mock_server = wiremock::MockServer::start().await;
    // TODO: Add mocked endpoints

    let mut harness = harness::Harness::builder()
        .seed(1337)
        .mocked_server(mock_server)
        .build();
    let cfg = harness.make_program_cfg();
    let stream_rx = harness.take_stream();
    let test_fut = async {
        let std_stream = stream_rx
            .recv_async()
            .await
            .wrap_err("failed to receive stream")??;
        std_stream
            .set_nonblocking(true)
            .wrap_err("failed to set nonblocking")?;
        let tokio_stream = tokio::net::UnixStream::from_std(std_stream)
            .wrap_err("failed to convert from std stream")?;

        do_test(tokio_stream).await.wrap_err("error in test")
    };
    let program_fut = async {
        orb_se050_reprovision::run(cfg)
            .await
            .wrap_err("error in program")
    };
    let ((), ()) = tokio::try_join!(test_fut, program_fut)?;

    Ok(())
}

async fn do_test(mut stream: tokio::net::UnixStream) -> Result<()> {
    stream.write_u8(69).await.wrap_err("failed to write u8")?;
    stream
        .shutdown()
        .await
        .wrap_err("failed to shut down stream")?;
    drop(stream);
    info!("dropped stream");

    Ok(())
}
