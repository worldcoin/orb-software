use eyre::WrapErr as _;
use orb_supervisor::{
    startup::{
        Application,
        Settings,
    },
    telemetry,
};
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    telemetry::start(LevelFilter::INFO, std::io::stdout);

    debug!("Starting orb supervisor");

    let settings = Settings::default();
    let application = Application::build(settings.clone())
        .await
        .wrap_err("failed to build supervisor")?;

    application.run().await?;

    Ok(())
}
