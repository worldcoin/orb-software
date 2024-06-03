use color_eyre::eyre::WrapErr as _;
use orb_supervisor::{
    startup::{Application, Settings},
    telemetry::{self, ExecContext},
};
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    telemetry::start::<ExecContext, _>(LevelFilter::INFO, std::io::stdout)
        .wrap_err("failed to initialize tracing; bailing")?;
    debug!("initialized telemetry");

    let settings = Settings::default();
    debug!(?settings, "starting supervisor with settings");
    let application = Application::build(settings.clone())
        .await
        .wrap_err("failed to build supervisor")?;

    application.run().await?;

    Ok(())
}
