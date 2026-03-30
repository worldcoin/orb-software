use color_eyre::{eyre::WrapErr as _, Result};
use rand::SeedableRng;

mod harness;

#[tokio::test]
async fn foobar() -> Result<()> {
    let _ = color_eyre::install();
    let mock_server = wiremock::MockServer::start().await;
    // TODO: Add mocked endpoints

    let harness = harness::Harness::builder()
        .seed(1337)
        .mocked_server(mock_server)
        .build();
    let cfg = harness.make_program_cfg();
    orb_se050_reprovision::run(cfg)
        .await
        .wrap_err("program error")?;

    Ok(())
}
