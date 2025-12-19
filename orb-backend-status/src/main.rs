use color_eyre::eyre::Result;
use orb_backend_status::backend::os_version::orb_os_version;
use orb_endpoints::{v2::Endpoints, Backend};
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::time::Duration;
use tokio::signal::unix::{self, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing::warn;

const SYSLOG_IDENTIFIER: &str = "worldcoin-backend-status";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

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

    // TODO: add better error context
    let orb_id = OrbId::read().await?;
    let endpoint = Endpoints::new(Backend::from_env()?, &orb_id).status;
    let endpoint = Url::parse(endpoint.as_str())?;

    let orb_name = OrbName::read().await.unwrap_or_else(|e| {
        warn!("failed to read orb name: {e:?}");
        OrbName("unknown".to_string())
    });
    let orb_jabil_id = OrbJabilId::read().await.unwrap_or_else(|e| {
        warn!("failed to read orb jabil id: {e:?}");
        OrbJabilId("unknown".to_string())
    });

    let result = orb_backend_status::program()
        .dbus(zbus::Connection::session().await?)
        .endpoint(endpoint)
        .orb_os_version(orb_os_version()?)
        .orb_id(orb_id)
        .orb_name(orb_name)
        .orb_jabil_id(orb_jabil_id)
        .procfs("/proc")
        .net_stats_poll_interval(Duration::from_secs(30))
        .connectivity_poll_interval(Duration::from_secs(2))
        .sender_interval(Duration::from_secs(30))
        .sender_min_backoff(Duration::from_secs(1))
        .sender_max_backoff(Duration::from_secs(30))
        .req_timeout(Duration::from_secs(5))
        .req_min_retry_interval(Duration::from_millis(100))
        .req_max_retry_interval(Duration::from_secs(2))
        .shutdown_token(shutdown_token)
        .run()
        .await;

    telemetry.flush().await;

    result
}
