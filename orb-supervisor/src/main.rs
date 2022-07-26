use eyre::WrapErr as _;
use orb_supervisor::{
    startup::{
        Application,
        Settings,
    },
    telemetry,
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    telemetry::start();

    let settings = Settings::default();
    let application = Application::build(settings.clone())
        .await
        .wrap_err("failed to build supervisor")?;

    application.run().await?;

    Ok(())
}
