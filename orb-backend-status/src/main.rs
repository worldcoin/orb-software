use color_eyre::eyre::Result;
use orb_dogd::DogstatsdClient;
use std::time::Duration;
use tokio::signal::unix::{self, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing::warn;

mod startup;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    #[cfg(all(feature = "android-collectors", not(feature = "linux-collectors")))]
    let args = <startup::android::Args as clap::Parser>::parse();

    let telemetry = orb_telemetry::TelemetryConfig::new();
    #[cfg(feature = "linux-collectors")]
    let telemetry = telemetry.with_journald("worldcoin-backend-status");
    let telemetry = telemetry.init();

    let shutdown_token = CancellationToken::new();

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;
    tokio::spawn({
        let shutdown_token = shutdown_token.clone();
        async move {
            tokio::select! {
                _ = sigterm.recv() => warn!("received SIGTERM"),
                _ = sigint.recv()  => warn!("received SIGINT"),
            }
            shutdown_token.cancel();
        }
    });

    #[cfg(feature = "linux-collectors")]
    let config = startup::linux::configure().await?;
    #[cfg(all(feature = "android-collectors", not(feature = "linux-collectors")))]
    let config = startup::android::configure(args).await?;

    let zsession = zenorb::Zenorb::from_cfg(config.zenoh)
        .orb_id(config.orb_id.clone())
        .with_name("orb-backend-status")
        .await?;

    let metrics = match config.metrics_socket {
        Some(socket) => DogstatsdClient::new_with(
            4096,
            25,
            Duration::from_millis(50),
            socket,
            Duration::from_secs(10),
        ),
        None => DogstatsdClient::default(),
    };

    let result = orb_backend_status::program()
        .metrics(metrics)
        .collector_config(config.collectors)
        .zsession(&zsession)
        .endpoint(config.endpoint)
        .orb_os_version(config.orb_os_version)
        .orb_id(config.orb_id)
        .orb_name(config.orb_name)
        .orb_jabil_id(config.orb_jabil_id)
        .procfs("/proc")
        .sender_interval(Duration::from_secs(30))
        .req_timeout(Duration::from_secs(2))
        .req_min_retry_interval(Duration::from_millis(100))
        .req_max_retry_interval(Duration::from_secs(500))
        .shutdown_token(shutdown_token)
        .run()
        .await;

    telemetry.flush().await;

    result
}
